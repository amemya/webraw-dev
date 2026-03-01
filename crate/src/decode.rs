/// RAW decode module
/// Wraps rawloader to decode RAW files from byte arrays and extract metadata.

use std::io::Cursor;
use crate::demosaic;

/// Metadata extracted from the RAW file
#[derive(serde::Serialize)]
pub struct RawMetadata {
    pub width: usize,
    pub height: usize,
    pub make: String,
    pub model: String,
    /// White balance coefficients [R, G, B, G2]
    pub wb_coeffs: [f32; 4],
    /// Black levels per CFA position
    pub black_levels: Vec<u16>,
    /// White levels per CFA position
    pub white_levels: Vec<u16>,
    /// CFA pattern as string (e.g., "RGGB")
    pub cfa_pattern: String,
    /// Camera-to-sRGB color matrix (flattened 3x3, row-major)
    pub color_matrix: Vec<f32>,
    /// Raw xyz_to_cam matrix from rawloader (for debugging)
    pub xyz_to_cam_raw: Vec<f32>,
}

/// Result of decoding a RAW file
pub struct DecodeResult {
    /// Demosaiced RGB data as f32 (0.0+), length = width * height * 3
    pub pixels: Vec<f32>,
    /// Metadata from the RAW file
    pub metadata: RawMetadata,
    /// If true, pixel data is display-referred (DCP tone curve applied);
    /// do NOT apply sRGB gamma. If false, data is linear and needs gamma.
    pub display_referred: bool,
}

/// Decode a RAW file from a byte slice
pub fn decode_raw_bytes(data: &[u8]) -> Result<DecodeResult, String> {
    let mut cursor = Cursor::new(data);
    let raw_image = rawloader::decode(&mut cursor).map_err(|e| format!("RAW decode error: {}", e))?;

    // Full sensor dimensions
    let full_width = raw_image.width;
    let full_height = raw_image.height;

    // Active area crop: [top, right, bottom, left]
    let crops = raw_image.crops;
    let crop_top = crops[0];
    let crop_right = crops[1];
    let crop_bottom = crops[2];
    let crop_left = crops[3];

    // Cropped (active area) dimensions
    let width = full_width - crop_left - crop_right;
    let height = full_height - crop_top - crop_bottom;

    // Extract CFA pattern — offset by crop to get correct alignment
    let cfa_pattern = parse_cfa_pattern_with_offset(&raw_image.cfa, crop_top, crop_left);
    let cfa_string = format_cfa_pattern(&cfa_pattern);

    // Extract black and white levels
    // Note: rawloader may return 0 for some CFA positions (e.g., blacklevels[3]=0
    // for Canon). Sensor values are actually ~2047, so a black level of 0 would
    // cause that channel to be grossly over-normalized. Fix by using the max
    // non-zero black level for any position that has 0.
    let raw_bl = &raw_image.blacklevels;
    let max_black = raw_bl.iter().cloned().fold(0u16, |a, b| a.max(b as u16));
    let black_levels: Vec<u16> = raw_bl.iter().map(|&x| {
        let v = x as u16;
        if v == 0 && max_black > 0 { max_black } else { v }
    }).collect();
    let white_levels: Vec<u16> = raw_image.whitelevels.iter().map(|&x| x as u16).collect();

    // Compute WB multipliers (simple approach — ForwardMatrix handles color space)
    let wb_coeffs = raw_image.wb_coeffs;
    let wb_mults = compute_wb_multipliers(&wb_coeffs, &cfa_pattern);

    // Extract raw xyz_to_cam for debugging
    let xtc = &raw_image.xyz_to_cam;
    let xyz_to_cam_raw: Vec<f32> = (0..3).flat_map(|r| xtc[r].iter().copied()).collect();

    // Normalize raw data, apply crops and WB pre-demosaic
    let normalized = normalize_crop_and_wb(
        &raw_image, &black_levels, &white_levels, &wb_mults,
        full_width, crop_top, crop_left, width, height,
    )?;

    // Demosaic (data is cropped and white-balanced)
    let mut rgb = demosaic::demosaic_bilinear(&normalized, width, height, &cfa_pattern);

    // Try to load bundled DCP profile and apply DNG color pipeline
    let dcp_profile = load_bundled_dcp(&raw_image.make, &raw_image.model);
    let (color_matrix, display_referred) = if let Some(ref profile) = dcp_profile {
        // Estimate color temperature from WB coefficients
        let temperature = estimate_color_temperature(&wb_coeffs);

        // Apply full DNG pipeline: ForwardMatrix → ProPhoto → LookTable → ToneCurve → sRGB
        // Output is display-referred — DCP tone curve IS the complete display encoding
        crate::color_pipeline::apply_dcp_pipeline(
            &mut rgb, width, height, profile, temperature,
        );

        // Return ForwardMatrix as debug info
        let matrix: Vec<f32> = profile.forward_matrix_2
            .unwrap_or(profile.forward_matrix_1.unwrap_or([0.0; 9]))
            .iter().map(|&x| x as f32).collect();
        (matrix, true)
    } else {
        // Fallback: simple matrix approach — output is linear, needs sRGB gamma
        let (_, cam_to_srgb) = compute_wb_and_matrix_fallback(&raw_image, &cfa_pattern);
        apply_color_matrix(&mut rgb, width, height, &cam_to_srgb);
        (cam_to_srgb.to_vec(), false)
    };

    let metadata = RawMetadata {
        width,
        height,
        make: raw_image.make.clone(),
        model: raw_image.model.clone(),
        wb_coeffs,
        black_levels,
        white_levels,
        cfa_pattern: cfa_string,
        color_matrix,
        xyz_to_cam_raw,
    };

    Ok(DecodeResult {
        pixels: rgb,
        metadata,
        display_referred,
    })
}

/// Compute simple WB multipliers from camera coefficients
fn compute_wb_multipliers(wb_coeffs: &[f32; 4], cfa_pattern: &[u8; 4]) -> [f32; 4] {
    let mut mults = [1.0f32; 4];
    for i in 0..4 {
        let color = cfa_pattern[i];
        let coeff = match color {
            0 => wb_coeffs[0],
            1 => {
                let g1 = wb_coeffs[1];
                let g2 = wb_coeffs[3];
                if g2.is_finite() && g2 > 0.0 { (g1 + g2) / 2.0 } else { g1 }
            }
            2 => wb_coeffs[2],
            _ => 1.0,
        };
        mults[i] = if coeff.is_finite() && coeff > 0.0 { coeff } else { 1.0 };
    }
    let min_wb = mults.iter().cloned().fold(f32::MAX, f32::min);
    if min_wb > 0.0 {
        for m in mults.iter_mut() { *m /= min_wb; }
    }
    mults
}

/// Load bundled DCP profile for a given camera make/model
fn load_bundled_dcp(make: &str, model: &str) -> Option<crate::dcp::DcpProfile> {
    // Bundled profiles — embedded at compile time
    let dcp_data: Option<&[u8]> = if model.contains("EOS-1D X") && !model.contains("Mark") {
        Some(include_bytes!("../../profiles/Canon EOS-1D X Camera Standard.dcp"))
    } else {
        None
    };

    dcp_data.and_then(|data| crate::dcp::parse_dcp(data).ok())
}

/// Estimate color temperature from WB coefficients (rough approximation)
fn estimate_color_temperature(wb_coeffs: &[f32; 4]) -> f64 {
    // R/B ratio roughly correlates with color temperature
    // Higher R/B = warmer light = lower Kelvin? No, higher R = more red compensation = cooler light
    let r = wb_coeffs[0];
    let b = wb_coeffs[2];
    if r <= 0.0 || b <= 0.0 { return 6500.0; }
    let ratio = r as f64 / b as f64;
    // Empirical mapping: ratio ~1.0 ≈ 5500K, ratio ~2.0 ≈ 2800K, ratio ~0.5 ≈ 10000K
    // Using inverse relationship: T ≈ 5500 / ratio
    (5500.0 / ratio).clamp(2000.0, 12000.0)
}

/// Fallback: compute WB and cam_to_srgb when no DCP profile is available
fn compute_wb_and_matrix_fallback(
    raw_image: &rawloader::RawImage,
    cfa_pattern: &[u8; 4],
) -> ([f32; 4], [f32; 9]) {
    let wb_coeffs = &raw_image.wb_coeffs;
    let xtc = &raw_image.xyz_to_cam;

    // Check if color matrix is populated
    let mut has_matrix = false;
    for r in 0..3 {
        for c in 0..3 {
            if xtc[r][c].abs() > 1.0 {
                has_matrix = true;
            }
        }
    }

    // Scale xyz_to_cam by 1/10000 and compute row sums
    let mut row_sums = [1.0f32; 3]; // R, G, B channel row sums
    let mut xyz_to_cam_3x3 = [[0.0f32; 3]; 3];

    if has_matrix {
        for r in 0..3 {
            let mut sum = 0.0f32;
            for c in 0..3 {
                xyz_to_cam_3x3[r][c] = xtc[r][c] / 10000.0;
                sum += xyz_to_cam_3x3[r][c];
            }
            row_sums[r] = if sum.abs() > 1e-10 { sum } else { 1.0 };
            // Normalize row to sum to 1
            for c in 0..3 {
                xyz_to_cam_3x3[r][c] /= row_sums[r];
            }
        }
    }

    // Compute WB multipliers — standard approach (no row-sum adjustment)
    // The row-sum adjustment over-boosts R. Standard WB with the row-normalized
    // matrix's natural color separation provides the best balance.
    let mut wb_mults = [1.0f32; 4];
    for i in 0..4 {
        let color = cfa_pattern[i]; // 0=R, 1=G, 2=B
        let coeff = match color {
            0 => wb_coeffs[0],
            1 => {
                let g1 = wb_coeffs[1];
                let g2 = wb_coeffs[3];
                if g2.is_finite() && g2 > 0.0 { (g1 + g2) / 2.0 } else { g1 }
            }
            2 => wb_coeffs[2],
            _ => 1.0,
        };
        wb_mults[i] = if coeff.is_finite() && coeff > 0.0 { coeff } else { 1.0 };
    }

    // Normalize by minimum (boost only, preserve dynamic range)
    let min_wb = wb_mults.iter().cloned().fold(f32::MAX, f32::min);
    if min_wb > 0.0 {
        for w in wb_mults.iter_mut() {
            *w /= min_wb;
        }
    }

    // Compute cam_to_srgb matrix (NOT further normalized — preserves saturation)
    let cam_to_srgb = if has_matrix {
        let cam_to_xyz = invert_3x3(&xyz_to_cam_3x3);

        // XYZ to sRGB (D65) standard matrix
        let xyz_to_srgb: [f32; 9] = [
             3.2404542, -1.5371385, -0.4985314,
            -0.9692660,  1.8760108,  0.0415560,
             0.0556434, -0.2040259,  1.0572252,
        ];

        // cam_to_srgb = xyz_to_srgb * cam_to_xyz
        let mut result = [0.0f32; 9];
        for r in 0..3 {
            for c in 0..3 {
                let mut sum = 0.0;
                for k in 0..3 {
                    sum += xyz_to_srgb[r * 3 + k] * cam_to_xyz[k][c];
                }
                result[r * 3 + c] = sum;
            }
        }
        result
    } else {
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    };

    (wb_mults, cam_to_srgb)
}

/// Parse CFA pattern with crop offset — ensures correct color alignment for cropped area
fn parse_cfa_pattern_with_offset(cfa: &rawloader::CFA, crop_top: usize, crop_left: usize) -> [u8; 4] {
    let mut pattern = [1u8; 4];
    for row in 0..2 {
        for col in 0..2 {
            let idx = row * 2 + col;
            let color = cfa.color_at(row + crop_top, col + crop_left);
            pattern[idx] = color as u8;
        }
    }
    pattern
}

/// Normalize raw sensor data, apply crop, and apply WB pre-demosaic.
fn normalize_crop_and_wb(
    raw_image: &rawloader::RawImage,
    black_levels: &[u16],
    white_levels: &[u16],
    wb_mults: &[f32; 4],
    full_width: usize,
    crop_top: usize,
    crop_left: usize,
    width: usize,
    height: usize,
) -> Result<Vec<f32>, String> {
    match &raw_image.data {
        rawloader::RawImageData::Integer(data) => {
            let mut normalized = Vec::with_capacity(width * height);

            for row in 0..height {
                for col in 0..width {
                    // Map cropped coordinates to full sensor coordinates
                    let full_row = row + crop_top;
                    let full_col = col + crop_left;
                    let idx = full_row * full_width + full_col;

                    // CFA position in the CROPPED image (which has correct alignment)
                    let cfa_idx = (row % 2) * 2 + (col % 2);

                    // Use CFA position in FULL image for black/white levels
                    let full_cfa_idx = (full_row % 2) * 2 + (full_col % 2);
                    let black = black_levels.get(full_cfa_idx).copied().unwrap_or(0) as f32;
                    let white = white_levels.get(full_cfa_idx).copied().unwrap_or(65535) as f32;
                    let wb = wb_mults[cfa_idx];

                    let raw_val = data[idx] as f32;
                    let norm = ((raw_val - black) / (white - black)) * wb;
                    normalized.push(norm.max(0.0));
                }
            }
            Ok(normalized)
        }
        rawloader::RawImageData::Float(data) => {
            let mut normalized = Vec::with_capacity(width * height);
            for row in 0..height {
                for col in 0..width {
                    let full_row = row + crop_top;
                    let full_col = col + crop_left;
                    let idx = full_row * full_width + full_col;
                    let cfa_idx = (row % 2) * 2 + (col % 2);
                    let wb = wb_mults[cfa_idx];
                    normalized.push((data[idx] * wb).max(0.0));
                }
            }
            Ok(normalized)
        }
    }
}

/// Apply a 3x3 color matrix to all pixels in-place.
fn apply_color_matrix(rgb: &mut [f32], width: usize, height: usize, matrix: &[f32; 9]) {
    for i in 0..(width * height) {
        let idx = i * 3;
        let r = rgb[idx];
        let g = rgb[idx + 1];
        let b = rgb[idx + 2];

        rgb[idx]     = (matrix[0] * r + matrix[1] * g + matrix[2] * b).max(0.0);
        rgb[idx + 1] = (matrix[3] * r + matrix[4] * g + matrix[5] * b).max(0.0);
        rgb[idx + 2] = (matrix[6] * r + matrix[7] * g + matrix[8] * b).max(0.0);
    }
}

/// Format CFA pattern array to string
fn format_cfa_pattern(pattern: &[u8; 4]) -> String {
    pattern
        .iter()
        .map(|&c| match c {
            0 => 'R',
            1 => 'G',
            2 => 'B',
            _ => '?',
        })
        .collect()
}

/// Invert a 3x3 matrix using cofactor method
fn invert_3x3(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if det.abs() < 1e-10 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }

    let inv_det = 1.0 / det;

    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ]
}
