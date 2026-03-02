/// Bayer demosaicing module
/// Converts single-channel Bayer pattern data to full RGB using Malvar-He-Cutler interpolation.

/// CFA (Color Filter Array) pattern mapping
/// Each position maps to: 0=Red, 1=Green, 2=Blue
#[derive(Debug, Clone, Copy)]
pub enum CfaColor {
    Red,
    Green,
    Blue,
}

/// Determine the color at a given CFA position
pub fn cfa_color_at(cfa_pattern: &[u8; 4], row: usize, col: usize) -> CfaColor {
    let r = row % 2;
    let c = col % 2;
    let idx = r * 2 + c;
    match cfa_pattern[idx] {
        0 => CfaColor::Red,
        1 => CfaColor::Green,
        2 => CfaColor::Blue,
        _ => CfaColor::Green, // fallback
    }
}

/// Convert rawloader CFA pattern string to our pattern array
pub fn parse_cfa_pattern(cfa: &rawloader::CFA) -> [u8; 4] {
    let mut pattern = [1u8; 4]; // default to green
    for row in 0..2 {
        for col in 0..2 {
            let idx = row * 2 + col;
            let color = cfa.color_at(row, col);
            pattern[idx] = color as u8;
        }
    }
    pattern
}

/// Malvar-He-Cutler (MHC) Demosaicing
/// High-Quality Linear Interpolation for Demosaicing of Color Images
pub fn demosaic_bilinear(
    raw: &[f32],
    width: usize,
    height: usize,
    cfa_pattern: &[u8; 4],
) -> Vec<f32> {
    let mut rgb = vec![0.0f32; width * height * 3];

    for row in 0..height {
        for col in 0..width {
            let pixel_idx = row * width + col;
            let out_idx = pixel_idx * 3;
            let color = cfa_color_at(cfa_pattern, row, col);

            match color {
                CfaColor::Red => {
                    rgb[out_idx] = raw[pixel_idx]; // R
                    rgb[out_idx + 1] = interpolate_g_at_r_or_b(raw, width, height, row, col).clamp(0.0, 1.0); // G
                    rgb[out_idx + 2] = interpolate_b_at_r(raw, width, height, row, col).clamp(0.0, 1.0); // B
                }
                CfaColor::Green => {
                    rgb[out_idx + 1] = raw[pixel_idx]; // G
                    let (r, b) = interpolate_rb_at_g(raw, width, height, row, col, cfa_pattern);
                    rgb[out_idx] = r.clamp(0.0, 1.0);
                    rgb[out_idx + 2] = b.clamp(0.0, 1.0);
                }
                CfaColor::Blue => {
                    rgb[out_idx + 2] = raw[pixel_idx]; // B
                    rgb[out_idx + 1] = interpolate_g_at_r_or_b(raw, width, height, row, col).clamp(0.0, 1.0); // G
                    rgb[out_idx] = interpolate_r_at_b(raw, width, height, row, col).clamp(0.0, 1.0); // R
                }
            }
        }
    }

    rgb
}

#[inline]
fn get_pixel(raw: &[f32], width: usize, height: usize, row: isize, col: isize) -> f32 {
    let r = row.clamp(0, height as isize - 1) as usize;
    let c = col.clamp(0, width as isize - 1) as usize;
    raw[r * width + c]
}

// Malvar-He-Cutler Filters

// G at R or B locations
fn interpolate_g_at_r_or_b(raw: &[f32], width: usize, height: usize, row: usize, col: usize) -> f32 {
    let r = row as isize;
    let c = col as isize;
    
    // Bilinear base for G
    let base = (get_pixel(raw, width, height, r - 1, c) +
                get_pixel(raw, width, height, r + 1, c) +
                get_pixel(raw, width, height, r, c - 1) +
                get_pixel(raw, width, height, r, c + 1)) / 4.0;
                
    // Laplacian correction (high-frequency detail) from the known center channel
    let center = get_pixel(raw, width, height, r, c);
    let laplacian = center - 0.25 * (
        get_pixel(raw, width, height, r - 2, c) +
        get_pixel(raw, width, height, r + 2, c) +
        get_pixel(raw, width, height, r, c - 2) +
        get_pixel(raw, width, height, r, c + 2)
    );
    
    // Calculate local contrast to attenuate sharpening near clipped edges
    let n1 = get_pixel(raw, width, height, r - 1, c);
    let n2 = get_pixel(raw, width, height, r + 1, c);
    let n3 = get_pixel(raw, width, height, r, c - 1);
    let n4 = get_pixel(raw, width, height, r, c + 1);
    let min_n = n1.min(n2).min(n3).min(n4).min(center);
    let max_n = n1.max(n2).max(n3).max(n4).max(center);
    let contrast = max_n - min_n;
    
    // Attenuate correction as contrast increases (protects highlights)
    let attenuation = (1.0 - contrast * 2.0).clamp(0.0, 1.0);
    let correction = laplacian * 0.5 * attenuation;
    
    (base + correction).clamp(0.0, 1.0)
}

// B at R location or R at B location (same filter shape)
fn interpolate_b_at_r(raw: &[f32], width: usize, height: usize, row: usize, col: usize) -> f32 {
    interpolate_diagonal(raw, width, height, row, col)
}

fn interpolate_r_at_b(raw: &[f32], width: usize, height: usize, row: usize, col: usize) -> f32 {
    interpolate_diagonal(raw, width, height, row, col)
}

fn interpolate_diagonal(raw: &[f32], width: usize, height: usize, row: usize, col: usize) -> f32 {
    let r = row as isize;
    let c = col as isize;
    
    // Bilinear base from the 4 diagonals
    let base = (get_pixel(raw, width, height, r - 1, c - 1) +
                get_pixel(raw, width, height, r - 1, c + 1) +
                get_pixel(raw, width, height, r + 1, c - 1) +
                get_pixel(raw, width, height, r + 1, c + 1)) / 4.0;
                
    // Laplacian correction from the known center channel
    let center = get_pixel(raw, width, height, r, c);
    let laplacian = center - 0.25 * (
        get_pixel(raw, width, height, r - 2, c) +
        get_pixel(raw, width, height, r + 2, c) +
        get_pixel(raw, width, height, r, c - 2) +
        get_pixel(raw, width, height, r, c + 2)
    );
    
    let n1 = get_pixel(raw, width, height, r - 1, c - 1);
    let n2 = get_pixel(raw, width, height, r - 1, c + 1);
    let n3 = get_pixel(raw, width, height, r + 1, c - 1);
    let n4 = get_pixel(raw, width, height, r + 1, c + 1);
    let min_n = n1.min(n2).min(n3).min(n4).min(center);
    let max_n = n1.max(n2).max(n3).max(n4).max(center);
    let contrast = max_n - min_n;
    
    let attenuation = (1.0 - contrast * 2.0).clamp(0.0, 1.0);
    let correction = laplacian * 0.75 * attenuation;
    
    (base + correction).clamp(0.0, 1.0)
}

// R and B at G location
fn interpolate_rb_at_g(raw: &[f32], width: usize, height: usize, row: usize, col: usize, cfa_pattern: &[u8; 4]) -> (f32, f32) {
    let r = row as isize;
    let c = col as isize;
    
    let check_row = if row > 0 { row - 1 } else { row + 1 };
    let above_color = cfa_color_at(cfa_pattern, check_row, col);
    
    let center = get_pixel(raw, width, height, r, c);

    // Compute generic vertical and horizontal bilinear bases
    let base_v = (get_pixel(raw, width, height, r - 1, c) + get_pixel(raw, width, height, r + 1, c)) / 2.0;
    let base_h = (get_pixel(raw, width, height, r, c - 1) + get_pixel(raw, width, height, r, c + 1)) / 2.0;
    
    // Laplacian correction from the known center green channel
    let laplacian_v = center - 0.5 * (get_pixel(raw, width, height, r - 2, c) + get_pixel(raw, width, height, r + 2, c));
    let laplacian_h = center - 0.5 * (get_pixel(raw, width, height, r, c - 2) + get_pixel(raw, width, height, r, c + 2));
    
    let nv1 = get_pixel(raw, width, height, r - 1, c);
    let nv2 = get_pixel(raw, width, height, r + 1, c);
    let nh1 = get_pixel(raw, width, height, r, c - 1);
    let nh2 = get_pixel(raw, width, height, r, c + 1);
    
    let min_v = nv1.min(nv2).min(center);
    let max_v = nv1.max(nv2).max(center);
    let min_h = nh1.min(nh2).min(center);
    let max_h = nh1.max(nh2).max(center);
    
    let contrast_v = max_v - min_v;
    let contrast_h = max_h - min_h;
    
    let atten_v = (1.0 - contrast_v * 2.0).clamp(0.0, 1.0);
    let atten_h = (1.0 - contrast_h * 2.0).clamp(0.0, 1.0);

    let corr_v = laplacian_v * 0.625 * atten_v;
    let corr_h = laplacian_h * 0.625 * atten_h;
    
    let val_v = (base_v + corr_v).clamp(0.0, 1.0);
    let val_h = (base_h + corr_h).clamp(0.0, 1.0);

    match above_color {
        CfaColor::Red => {
            // R is vertical, B is horizontal
            (val_v, val_h)
        },
        CfaColor::Blue => {
            // B is vertical, R is horizontal
            (val_h, val_v)
        },
        CfaColor::Green => {
            // Fallback for safety, check left neighbor
            let check_col = if col > 0 { col - 1 } else { col + 1 };
            let left_color = cfa_color_at(cfa_pattern, row, check_col);
            match left_color {
                CfaColor::Red => {
                    // R is horizontal, B is vertical
                    (val_h, val_v)
                },
                _ => {
                    // B is horizontal, R is vertical
                    (val_v, val_h)
                }
            }
        }
    }
}
