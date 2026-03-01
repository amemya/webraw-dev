/// Bayer demosaicing module
/// Converts single-channel Bayer pattern data to full RGB using bilinear interpolation.

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
    let idx = (row % 2) * 2 + (col % 2);
    match cfa_pattern[idx] {
        0 => CfaColor::Red,
        1 => CfaColor::Green,
        2 => CfaColor::Blue,
        _ => CfaColor::Green, // fallback
    }
}

/// Convert rawloader CFA pattern string to our pattern array
/// rawloader CFA patterns: "RGGB", "BGGR", "GRBG", "GBRG"
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

/// Bilinear demosaicing
///
/// Takes raw Bayer data (f32, normalized 0.0-1.0) and produces RGB interleaved output.
/// Output: Vec<f32> with length = width * height * 3 (RGB planar per pixel)
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
                    // Red pixel: R is known, interpolate G and B
                    rgb[out_idx] = raw[pixel_idx]; // R
                    rgb[out_idx + 1] = interpolate_green_at_rb(raw, width, height, row, col); // G
                    rgb[out_idx + 2] = interpolate_blue_at_red(raw, width, height, row, col, cfa_pattern); // B
                }
                CfaColor::Green => {
                    // Green pixel: G is known, interpolate R and B
                    rgb[out_idx + 1] = raw[pixel_idx]; // G
                    let (r, b) = interpolate_rb_at_green(raw, width, height, row, col, cfa_pattern);
                    rgb[out_idx] = r;
                    rgb[out_idx + 2] = b;
                }
                CfaColor::Blue => {
                    // Blue pixel: B is known, interpolate R and G
                    rgb[out_idx + 2] = raw[pixel_idx]; // B
                    rgb[out_idx + 1] = interpolate_green_at_rb(raw, width, height, row, col); // G
                    rgb[out_idx] = interpolate_red_at_blue(raw, width, height, row, col, cfa_pattern); // R
                }
            }
        }
    }

    rgb
}

/// Safe pixel access with boundary clamping
#[inline]
fn get_pixel(raw: &[f32], width: usize, height: usize, row: isize, col: isize) -> f32 {
    let r = row.clamp(0, height as isize - 1) as usize;
    let c = col.clamp(0, width as isize - 1) as usize;
    raw[r * width + c]
}

/// Interpolate green at a red or blue pixel position (cross pattern)
fn interpolate_green_at_rb(raw: &[f32], width: usize, height: usize, row: usize, col: usize) -> f32 {
    let r = row as isize;
    let c = col as isize;
    let sum = get_pixel(raw, width, height, r - 1, c)
        + get_pixel(raw, width, height, r + 1, c)
        + get_pixel(raw, width, height, r, c - 1)
        + get_pixel(raw, width, height, r, c + 1);
    sum / 4.0
}

/// Interpolate blue at a red pixel (diagonal pattern)
fn interpolate_blue_at_red(
    raw: &[f32],
    width: usize,
    height: usize,
    row: usize,
    col: usize,
    _cfa_pattern: &[u8; 4],
) -> f32 {
    let r = row as isize;
    let c = col as isize;
    let sum = get_pixel(raw, width, height, r - 1, c - 1)
        + get_pixel(raw, width, height, r - 1, c + 1)
        + get_pixel(raw, width, height, r + 1, c - 1)
        + get_pixel(raw, width, height, r + 1, c + 1);
    sum / 4.0
}

/// Interpolate red at a blue pixel (diagonal pattern)
fn interpolate_red_at_blue(
    raw: &[f32],
    width: usize,
    height: usize,
    row: usize,
    col: usize,
    _cfa_pattern: &[u8; 4],
) -> f32 {
    let r = row as isize;
    let c = col as isize;
    let sum = get_pixel(raw, width, height, r - 1, c - 1)
        + get_pixel(raw, width, height, r - 1, c + 1)
        + get_pixel(raw, width, height, r + 1, c - 1)
        + get_pixel(raw, width, height, r + 1, c + 1);
    sum / 4.0
}

/// Interpolate R and B at a green pixel position
fn interpolate_rb_at_green(
    raw: &[f32],
    width: usize,
    height: usize,
    row: usize,
    col: usize,
    cfa_pattern: &[u8; 4],
) -> (f32, f32) {
    let r = row as isize;
    let c = col as isize;

    // Determine which neighbors are R and which are B
    // Check if the pixel above is Red or Blue
    let above_color = cfa_color_at(cfa_pattern, row.wrapping_sub(1), col);

    match above_color {
        CfaColor::Red => {
            // R is above/below, B is left/right
            let red = (get_pixel(raw, width, height, r - 1, c)
                + get_pixel(raw, width, height, r + 1, c))
                / 2.0;
            let blue = (get_pixel(raw, width, height, r, c - 1)
                + get_pixel(raw, width, height, r, c + 1))
                / 2.0;
            (red, blue)
        }
        CfaColor::Blue => {
            // B is above/below, R is left/right
            let blue = (get_pixel(raw, width, height, r - 1, c)
                + get_pixel(raw, width, height, r + 1, c))
                / 2.0;
            let red = (get_pixel(raw, width, height, r, c - 1)
                + get_pixel(raw, width, height, r, c + 1))
                / 2.0;
            (red, blue)
        }
        CfaColor::Green => {
            // Check left neighbor instead
            let left_color = cfa_color_at(cfa_pattern, row, col.wrapping_sub(1));
            match left_color {
                CfaColor::Red => {
                    let red = (get_pixel(raw, width, height, r, c - 1)
                        + get_pixel(raw, width, height, r, c + 1))
                        / 2.0;
                    let blue = (get_pixel(raw, width, height, r - 1, c)
                        + get_pixel(raw, width, height, r + 1, c))
                        / 2.0;
                    (red, blue)
                }
                _ => {
                    let blue = (get_pixel(raw, width, height, r, c - 1)
                        + get_pixel(raw, width, height, r, c + 1))
                        / 2.0;
                    let red = (get_pixel(raw, width, height, r - 1, c)
                        + get_pixel(raw, width, height, r + 1, c))
                        / 2.0;
                    (red, blue)
                }
            }
        }
    }
}
