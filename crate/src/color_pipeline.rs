/// DNG Color Pipeline
///
/// Implements the full DNG color conversion using a DCP profile:
/// raw → WB → demosaic → ForwardMatrix → XYZ D50 → ProPhoto RGB
///     → HueSatMap/LookTable (3D LUT in HSV) → ToneCurve → sRGB

use crate::dcp::{DcpProfile, HueSatMapDims, HueSatMapEntry};

// ProPhoto RGB to XYZ D50
const PROPHOTO_TO_XYZ: [f64; 9] = [
    0.7976749, 0.1351917, 0.0313534,
    0.2880402, 0.7118741, 0.0000857,
    0.0000000, 0.0000000, 0.8252100,
];

// XYZ D50 to ProPhoto RGB
const XYZ_TO_PROPHOTO: [f64; 9] = [
    1.3459433, -0.2556075, -0.0511118,
    -0.5445989, 1.5081673,  0.0205351,
     0.0000000, 0.0000000,  1.2118128,
];

// XYZ D50 to sRGB D65 (with Bradford chromatic adaptation D50→D65)
const XYZ_D50_TO_SRGB: [f64; 9] = [
     3.1338561, -1.6168667, -0.4906146,
    -0.9787684,  1.9161415,  0.0334540,
     0.0719453, -0.2289914,  1.4052427,
];

/// Apply the DNG color pipeline to demosaiced, WB'd camera RGB data.
///
/// `pixels`: mutable RGB f32 data (WB'd, demosaiced, linear camera space)
/// `profile`: parsed DCP profile
/// `temperature`: estimated scene color temperature in Kelvin (for interpolation)
///
/// **Output**: display-referred sRGB values (0.0–1.0).
/// The DCP tone curve serves as the complete display encoding.
/// Do NOT apply sRGB gamma afterwards.
pub fn apply_dcp_pipeline(
    pixels: &mut [f32],
    width: usize,
    height: usize,
    profile: &DcpProfile,
    temperature: f64,
    wb_mults: &[f32; 3],
) {
    // 1. Compute interpolated ForwardMatrix based on color temperature
    let forward_matrix = interpolate_matrix(profile, temperature);

    // 2. Compute combined camera → ProPhoto RGB matrix
    //    cam_to_prophoto = XYZ_to_ProPhoto * ForwardMatrix
    let cam_to_prophoto = mat_mul_3x3(&XYZ_TO_PROPHOTO, &forward_matrix);

    // 3. Compute combined ProPhoto → sRGB (linear) matrix
    let prophoto_to_srgb = mat_mul_3x3(&XYZ_D50_TO_SRGB, &PROPHOTO_TO_XYZ);

    // Baseline exposure offset (convert from EV to linear multiplier)
    let baseline_exposure = profile.baseline_exposure_offset.unwrap_or(0.0);
    // Remove the arbitrary +2.0 EV boost to match actual camera standard brightness
    let total_exposure = baseline_exposure;
    let exposure_multiplier = 2.0f64.powf(total_exposure);

    // Get the WB multipliers to know the clip points
    let wb_r = wb_mults[0] as f64;
    let wb_g = wb_mults[1] as f64;
    let wb_b = wb_mults[2] as f64;

    // 5. Process each pixel
    //    Pipeline: CamRGB → ProPhoto → HSV(LookTable) → ProPhoto → Exposure Boost → sRGB(linear, via matrix)
    for i in 0..(width * height) {
        let idx = i * 3;
        let mut r = pixels[idx] as f64;
        let mut g = pixels[idx + 1] as f64;
        let mut b = pixels[idx + 2] as f64;

        let mut norm_r = r / wb_r;
        let mut norm_g = g / wb_g;
        let mut norm_b = b / wb_b;
        
        // --- Hue-Preserving Highlight Reconstruction (Camera Space) ---
        // Yellow bokehs turning orange happen because R and G clip at sensor max (1.0), 
        // but applying White Balance scales them differently (e.g., R x 2.5, G x 1.0),
        // changing the physical hue ratio towards red/orange.
        
        let max_norm = norm_r.max(norm_g).max(norm_b);
        let clip_thresh = 0.95; 

        if max_norm > clip_thresh {
            let blend = ((max_norm - clip_thresh) / (1.0 - clip_thresh)).clamp(0.0, 1.0);
            let smooth_blend = blend * blend * (3.0 - 2.0 * blend);
            
            // Just desaturate towards Luma in WB-applied space to avoid out-of-bounds math
            let luma = r * 0.299 + g * 0.587 + b * 0.114;
            r = r * (1.0 - smooth_blend) + luma * smooth_blend;
            g = g * (1.0 - smooth_blend) + luma * smooth_blend;
            b = b * (1.0 - smooth_blend) + luma * smooth_blend;
        }

        // Camera RGB → ProPhoto RGB via ForwardMatrix
        let mut pr = cam_to_prophoto[0] * r + cam_to_prophoto[1] * g + cam_to_prophoto[2] * b;
        let mut pg = cam_to_prophoto[3] * r + cam_to_prophoto[4] * g + cam_to_prophoto[5] * b;
        let mut pb = cam_to_prophoto[6] * r + cam_to_prophoto[7] * g + cam_to_prophoto[8] * b;

        // Prevent underflow artifacts (black pixels) from negative matrix math on out-of-gamut clipped colors
        pr = pr.max(0.0);
        pg = pg.max(0.0);
        pb = pb.max(0.0);

        let col = (idx / 3) % width;
        let row = (idx / 3) / width;
        if col >= 2480 && col <= 2490 && row == 1160 {
            println!("MAT({},{}): in({:.4},{:.4},{:.4}) -> prophoto({:.4},{:.4},{:.4})", col, row, r, g, b, pr, pg, pb);
        }

        let mut pr2 = pr;
        let mut pg2 = pg;
        let mut pb2 = pb;

        let desat_start = 0.85; 
        let desat_end = 0.98;
        let mut lut_blend = 1.0;
        
        if max_norm > desat_start {
            let mut blend = ((max_norm - desat_start) / (desat_end - desat_start)).clamp(0.0, 1.0);
            blend = blend * blend * (3.0 - 2.0 * blend);
            
            // Fade out LUT effect entirely for highlights to prevent weird hue shifts
            lut_blend = 1.0 - blend;
        }

        // 1. Apply HueSatMap FIRST
            if let Some(ref dims) = profile.hue_sat_map_dims {
                let hsm_data = interpolate_hue_sat_map(profile, temperature);
                if !hsm_data.is_empty() {
                    let mut pr_h = pr2; let mut pg_h = pg2; let mut pb_h = pb2;
                    if profile.hue_sat_map_encoding == 1 {
                        let pr_g = linear_to_srgb_f64(pr_h);
                        let pg_g = linear_to_srgb_f64(pg_h);
                        let pb_g = linear_to_srgb_f64(pb_h);
                        let (mut h, mut s, mut v) = rgb_to_hsv(pr_g, pg_g, pb_g);
                        apply_hue_sat_map(dims, &hsm_data, &mut h, &mut s, &mut v);
                        let (rg, gg, bg) = hsv_to_rgb(h, s, v);
                        pr_h = srgb_to_linear_f64(rg);
                        pg_h = srgb_to_linear_f64(gg);
                        pb_h = srgb_to_linear_f64(bg);
                    } else {
                        let (mut h, mut s, mut v) = rgb_to_hsv(pr_h, pg_h, pb_h);
                        apply_hue_sat_map(dims, &hsm_data, &mut h, &mut s, &mut v);
                        let (r2, g2, b2) = hsv_to_rgb(h, s, v);
                        pr_h = r2; pg_h = g2; pb_h = b2;
                    }
                    pr2 = pr2 * (1.0 - lut_blend) + pr_h * lut_blend;
                    pg2 = pg2 * (1.0 - lut_blend) + pg_h * lut_blend;
                    pb2 = pb2 * (1.0 - lut_blend) + pb_h * lut_blend;
                }
            }

            // 2. Apply LookTable NEXT
            if let (Some(ref dims), Some(ref data)) = (&profile.look_table_dims, &profile.look_table_data) {
                let mut pr_h = pr2; let mut pg_h = pg2; let mut pb_h = pb2;
                if profile.look_table_encoding == 1 {
                    let pr_g = linear_to_srgb_f64(pr_h);
                    let pg_g = linear_to_srgb_f64(pg_h);
                    let pb_g = linear_to_srgb_f64(pb_h);
                    let (mut h, mut s, mut v) = rgb_to_hsv(pr_g, pg_g, pb_g);
                    apply_hue_sat_map(dims, data, &mut h, &mut s, &mut v);
                    let (rg, gg, bg) = hsv_to_rgb(h, s, v);
                    pr_h = srgb_to_linear_f64(rg);
                    pg_h = srgb_to_linear_f64(gg);
                    pb_h = srgb_to_linear_f64(bg);
                } else {
                    let (mut h, mut s, mut v) = rgb_to_hsv(pr_h, pg_h, pb_h);
                    apply_hue_sat_map(dims, data, &mut h, &mut s, &mut v);
                    let (r2, g2, b2) = hsv_to_rgb(h, s, v);
                    pr_h = r2; pg_h = g2; pb_h = b2;
                }
                pr2 = pr2 * (1.0 - lut_blend) + pr_h * lut_blend;
                pg2 = pg2 * (1.0 - lut_blend) + pg_h * lut_blend;
                pb2 = pb2 * (1.0 - lut_blend) + pb_h * lut_blend;
            }
        // 3. Convert back to Linear sRGB
        let pr_lin = pr2;
        let pg_lin = pg2;
        let pb_lin = pb2;

        // 5. Apply ProPhoto → sRGB (linear) matrix
        let mut sr = prophoto_to_srgb[0] * pr_lin + prophoto_to_srgb[1] * pg_lin + prophoto_to_srgb[2] * pb_lin;
        let mut sg = prophoto_to_srgb[3] * pr_lin + prophoto_to_srgb[4] * pg_lin + prophoto_to_srgb[5] * pb_lin;
        let mut sb = prophoto_to_srgb[6] * pr_lin + prophoto_to_srgb[7] * pg_lin + prophoto_to_srgb[8] * pb_lin;

        // Prevent negative values from out-of-gamut colors before applying exposure
        // Negative linear values cannot be represented and cause zero-clipping artifacts in sRGB
        sr = sr.max(0.0);
        sg = sg.max(0.0);
        sb = sb.max(0.0);

        // 6. Output Linear sRGB with Baseline Exposure offset
        // We do *not* apply sRGB gamma here because the pipeline expects linear sRGB
        // so that subsequent filters (exposure, contrast) and final gamma encoding
        // can be properly applied by the WebGPU shader or raw2ppm tool.
        pixels[idx] = (sr * exposure_multiplier) as f32;
        pixels[idx + 1] = (sg * exposure_multiplier) as f32;
        pixels[idx + 2] = (sb * exposure_multiplier) as f32;
    }
}

/// Interpolate ForwardMatrix between illuminant1 and illuminant2 based on temperature
fn interpolate_matrix(profile: &DcpProfile, temperature: f64) -> [f64; 9] {
    let fm1 = profile.forward_matrix_1.unwrap_or_else(|| identity_matrix());
    let fm2 = profile.forward_matrix_2.unwrap_or(fm1);

    let t1 = profile.illuminant1.temperature();
    let t2 = profile.illuminant2.temperature();

    if (t2 - t1).abs() < 1.0 {
        return fm2; // same illuminant, no interpolation
    }

    // Interpolation weight: 0.0 = illuminant1, 1.0 = illuminant2
    // Use inverse temperature (mireds) for perceptually linear interpolation
    let mired1 = 1e6 / t1;
    let mired2 = 1e6 / t2;
    let mired_t = 1e6 / temperature.clamp(t1.min(t2), t1.max(t2));

    let w = if (mired2 - mired1).abs() < 0.01 {
        1.0
    } else {
        ((mired_t - mired1) / (mired2 - mired1)).clamp(0.0, 1.0)
    };

    let mut result = [0.0f64; 9];
    for i in 0..9 {
        result[i] = fm1[i] * (1.0 - w) + fm2[i] * w;
    }
    result
}

/// Interpolate HueSatMap data between two illuminants
fn interpolate_hue_sat_map(profile: &DcpProfile, temperature: f64) -> Vec<HueSatMapEntry> {
    let data1 = match &profile.hue_sat_map_data_1 {
        Some(d) => d,
        None => return Vec::new(),
    };

    let data2 = match &profile.hue_sat_map_data_2 {
        Some(d) => d,
        None => return data1.clone(),
    };

    if data1.len() != data2.len() {
        return data1.clone();
    }

    let t1 = profile.illuminant1.temperature();
    let t2 = profile.illuminant2.temperature();
    let mired1 = 1e6 / t1;
    let mired2 = 1e6 / t2;
    let mired_t = 1e6 / temperature.clamp(t1.min(t2), t1.max(t2));
    let w = if (mired2 - mired1).abs() < 0.01 {
        1.0
    } else {
        ((mired_t - mired1) / (mired2 - mired1)).clamp(0.0, 1.0)
    };

    data1.iter().zip(data2.iter()).map(|(a, b)| {
        HueSatMapEntry {
            hue_shift: a.hue_shift * (1.0 - w as f32) + b.hue_shift * w as f32,
            sat_scale: a.sat_scale * (1.0 - w as f32) + b.sat_scale * w as f32,
            val_scale: a.val_scale * (1.0 - w as f32) + b.val_scale * w as f32,
        }
    }).collect()
}

/// Apply a 3D HueSatMap LUT with trilinear interpolation
fn apply_hue_sat_map(
    dims: &HueSatMapDims,
    data: &[HueSatMapEntry],
    h: &mut f64,
    s: &mut f64,
    v: &mut f64,
) {
    let hue_divs = dims.hue_divs as f64;
    let sat_divs = dims.sat_divs as f64;
    let val_divs = dims.val_divs as f64;

    if hue_divs < 1.0 || sat_divs < 1.0 || val_divs < 1.0 {
        return;
    }

    // Map hue (0-360) to table index
    let hue_scaled = (*h / 360.0 * hue_divs).rem_euclid(hue_divs);
    let hue_idx0 = hue_scaled.floor() as usize;
    let hue_frac = hue_scaled - hue_idx0 as f64;
    let hue_idx1 = (hue_idx0 + 1) % dims.hue_divs as usize;

    // Map saturation (0-1) to table index
    let sat_scaled = (*s * (sat_divs - 1.0)).clamp(0.0, sat_divs - 1.001);
    let sat_idx0 = sat_scaled.floor() as usize;
    let sat_frac = sat_scaled - sat_idx0 as f64;
    let sat_idx1 = (sat_idx0 + 1).min(dims.sat_divs as usize - 1);

    // Map value (0-1) to table index
    let (val_idx0, val_idx1, val_frac) = if dims.val_divs <= 1 {
        (0usize, 0usize, 0.0f64)
    } else {
        let val_scaled = (*v * (val_divs - 1.0)).clamp(0.0, val_divs - 1.001);
        let vi0 = val_scaled.floor() as usize;
        let vf = val_scaled - vi0 as f64;
        let vi1 = (vi0 + 1).min(dims.val_divs as usize - 1);
        (vi0, vi1, vf)
    };

    // Trilinear interpolation
    let sd = dims.sat_divs as usize;
    let vd = dims.val_divs as usize;
    let hd = dims.hue_divs as usize; // Added Hue multiplier

    let entry_at = |hi: usize, si: usize, vi: usize| -> &HueSatMapEntry {
        // DNG Spec 1.4: "Value-major, Saturation-minor, Hue-micro"
        let idx = (vi * sd + si) * hd + hi;
        &data[idx.min(data.len() - 1)]
    };

    // Interpolate over value
    let interp_val = |hi: usize, si: usize| -> HueSatMapEntry {
        let e0 = entry_at(hi, si, val_idx0);
        let e1 = entry_at(hi, si, val_idx1);
        HueSatMapEntry {
            hue_shift: e0.hue_shift as f32 + (e1.hue_shift - e0.hue_shift) as f32 * val_frac as f32,
            sat_scale: e0.sat_scale as f32 + (e1.sat_scale - e0.sat_scale) as f32 * val_frac as f32,
            val_scale: e0.val_scale as f32 + (e1.val_scale - e0.val_scale) as f32 * val_frac as f32,
        }
    };

    // Interpolate over saturation
    let interp_sat = |hi: usize| -> HueSatMapEntry {
        let e0 = interp_val(hi, sat_idx0);
        let e1 = interp_val(hi, sat_idx1);
        HueSatMapEntry {
            hue_shift: e0.hue_shift + (e1.hue_shift - e0.hue_shift) * sat_frac as f32,
            sat_scale: e0.sat_scale + (e1.sat_scale - e0.sat_scale) * sat_frac as f32,
            val_scale: e0.val_scale + (e1.val_scale - e0.val_scale) * sat_frac as f32,
        }
    };

    // Interpolate over hue
    let e0 = interp_sat(hue_idx0);
    let e1 = interp_sat(hue_idx1);
    let mut final_entry = HueSatMapEntry {
        hue_shift: e0.hue_shift + (e1.hue_shift - e0.hue_shift) * hue_frac as f32,
        sat_scale: e0.sat_scale + (e1.sat_scale - e0.sat_scale) * hue_frac as f32,
        val_scale: e0.val_scale + (e1.val_scale - e0.val_scale) * hue_frac as f32,
    };

    // Fade out effect for extreme highlights (V > 0.85)
    // DCP LUTs are often extremely non-linear near the white point, causing luminance dips
    // that manifest as rings/banding when combined with local demosaic noise.
    let val_fade = if *v > 0.85 {
        (1.0 - (*v - 0.85) / 0.15).clamp(0.0, 1.0) as f32
    } else {
        1.0
    };
    
    final_entry.hue_shift *= val_fade;
    final_entry.sat_scale = 1.0 + (final_entry.sat_scale - 1.0) * val_fade;
    final_entry.val_scale = 1.0 + (final_entry.val_scale - 1.0) * val_fade;

    // Apply adjustments
    // hue_shift: additive offset in degrees
    // sat_scale/val_scale: direct multipliers (1.0 = no change)
    *h = (*h + final_entry.hue_shift as f64).rem_euclid(360.0);
    *s = (*s * final_entry.sat_scale as f64).clamp(0.0, 1.0);
    *v = (*v * final_entry.val_scale as f64).max(0.0);
}

/// Build a 4096-entry tone curve LUT for fast lookup
fn build_tone_lut(profile: &DcpProfile) -> Vec<f64> {
    let curve = match &profile.tone_curve {
        Some(c) if c.len() >= 2 => c,
        _ => {
            // Linear (identity) curve
            return (0..4096).map(|i| i as f64 / 4095.0).collect();
        }
    };

    let lut_size = 4096;
    let mut lut = vec![0.0f64; lut_size];

    for i in 0..lut_size {
        let x = i as f64 / (lut_size - 1) as f64;

        // Find the two surrounding curve points
        let mut lo = 0;
        let mut hi = curve.len() - 1;
        for j in 0..curve.len() - 1 {
            if curve[j].0 as f64 <= x && curve[j + 1].0 as f64 >= x {
                lo = j;
                hi = j + 1;
                break;
            }
        }

        let x0 = curve[lo].0 as f64;
        let y0 = curve[lo].1 as f64;
        let x1 = curve[hi].0 as f64;
        let y1 = curve[hi].1 as f64;

        lut[i] = if (x1 - x0).abs() < 1e-10 {
            y0
        } else {
            y0 + (y1 - y0) * (x - x0) / (x1 - x0)
        };
    }

    lut
}

/// Build a 4096-entry tone curve LUT as f32 for exporting to WebGPU
pub fn build_tone_lut_f32(profile: &DcpProfile) -> Vec<f32> {
    let lut_f64 = build_tone_lut(profile);
    lut_f64.into_iter().map(|v| v as f32).collect()
}

/// Apply tone curve via LUT lookup
fn apply_tone_curve(lut: &[f64], value: f64) -> f64 {
    if lut.is_empty() { return value; }
    
    if value <= 0.0 {
        return value;
    }
    
    if value >= 1.0 {
        let last_idx = lut.len() - 1;
        let dy = lut[last_idx] - lut[last_idx - 1];
        let dx = 1.0 / (lut.len() - 1) as f64;
        let slope = dy / dx;
        return lut[last_idx] + (value - 1.0) * slope;
    }

    let idx_f = value * (lut.len() - 1) as f64;
    let idx0 = idx_f.floor() as usize;
    let idx1 = (idx0 + 1).min(lut.len() - 1);
    let frac = idx_f - idx0 as f64;
    lut[idx0] * (1.0 - frac) + lut[idx1] * frac
}

// ---- Color space conversion helpers ----

fn rgb_to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let v = max;
    let s = if max > 1e-10 { delta / max } else { 0.0 };

    let h = if delta < 1e-10 {
        0.0
    } else if (max - r).abs() < 1e-10 {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() < 1e-10 {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };

    (h.rem_euclid(360.0), s, v)
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    if s < 1e-10 {
        return (v, v, v);
    }

    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (r1 + m, g1 + m, b1 + m)
}

fn mat_mul_3x3(a: &[f64; 9], b: &[f64; 9]) -> [f64; 9] {
    let mut r = [0.0f64; 9];
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                r[i * 3 + j] += a[i * 3 + k] * b[k * 3 + j];
            }
        }
    }
    r
}

fn identity_matrix() -> [f64; 9] {
    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
}

/// sRGB gamma: linear → display-referred (f64)
fn linear_to_srgb_f64(c: f64) -> f64 {
    let c = c.max(0.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Inverse sRGB gamma: display-referred → linear (f64)
fn srgb_to_linear_f64(c: f64) -> f64 {
    let c = c.max(0.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
