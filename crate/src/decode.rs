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
    /// Black levels per channel
    pub black_levels: Vec<u16>,
    /// White levels per channel
    pub white_levels: Vec<u16>,
    /// CFA pattern as string (e.g., "RGGB")
    pub cfa_pattern: String,
    /// Camera-to-XYZ color matrix (flattened 3x3 or 3x4)
    pub color_matrix: Vec<f32>,
}

/// Result of decoding a RAW file
pub struct DecodeResult {
    /// Demosaiced RGB data as f32 (0.0 - 1.0), length = width * height * 3
    pub pixels: Vec<f32>,
    /// Metadata from the RAW file
    pub metadata: RawMetadata,
}

/// Decode a RAW file from a byte slice
pub fn decode_raw_bytes(data: &[u8]) -> Result<DecodeResult, String> {
    let mut cursor = Cursor::new(data);
    let raw_image = rawloader::decode(&mut cursor).map_err(|e| format!("RAW decode error: {}", e))?;

    let width = raw_image.width;
    let height = raw_image.height;

    // Extract CFA pattern
    let cfa_pattern = demosaic::parse_cfa_pattern(&raw_image.cfa);
    let cfa_string = format_cfa_pattern(&cfa_pattern);

    // Extract black and white levels
    let black_levels: Vec<u16> = raw_image.blacklevels.iter().map(|&x| x as u16).collect();
    let white_levels: Vec<u16> = raw_image.whitelevels.iter().map(|&x| x as u16).collect();

    // Normalize raw data to f32 (0.0 - 1.0)
    let normalized = normalize_raw_data(&raw_image, &black_levels, &white_levels)?;

    // Demosaic
    let rgb = demosaic::demosaic_bilinear(&normalized, width, height, &cfa_pattern);

    // Build color matrix from camera metadata
    let color_matrix = extract_color_matrix(&raw_image);

    let metadata = RawMetadata {
        width,
        height,
        make: raw_image.make.clone(),
        model: raw_image.model.clone(),
        wb_coeffs: raw_image.wb_coeffs,
        black_levels,
        white_levels,
        cfa_pattern: cfa_string,
        color_matrix,
    };

    Ok(DecodeResult {
        pixels: rgb,
        metadata,
    })
}

/// Normalize raw sensor data to f32 [0.0, 1.0] range
/// Applies black level subtraction and white level normalization
fn normalize_raw_data(
    raw_image: &rawloader::RawImage,
    black_levels: &[u16],
    white_levels: &[u16],
) -> Result<Vec<f32>, String> {
    let width = raw_image.width;
    let height = raw_image.height;

    match &raw_image.data {
        rawloader::RawImageData::Integer(data) => {
            let mut normalized = Vec::with_capacity(width * height);

            for row in 0..height {
                for col in 0..width {
                    let idx = row * width + col;
                    let cfa_idx = (row % 2) * 2 + (col % 2);

                    let black = black_levels.get(cfa_idx).copied().unwrap_or(0) as f32;
                    let white = white_levels.get(cfa_idx).copied().unwrap_or(65535) as f32;

                    let raw_val = data[idx] as f32;
                    let norm = ((raw_val - black) / (white - black)).clamp(0.0, 1.0);
                    normalized.push(norm);
                }
            }
            Ok(normalized)
        }
        rawloader::RawImageData::Float(data) => {
            let mut normalized = Vec::with_capacity(width * height);
            for &val in data.iter() {
                normalized.push(val.clamp(0.0, 1.0));
            }
            Ok(normalized)
        }
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

/// Extract color matrix from the raw image
/// Returns the forward matrix or a default sRGB D65 matrix
fn extract_color_matrix(raw_image: &rawloader::RawImage) -> Vec<f32> {
    // rawloader stores color matrix in xyz_to_cam
    // We need cam_to_xyz which is the inverse
    // For now, return a reasonable default (sRGB D65)
    // This will be refined when applying the color pipeline in WebGPU
    let _ = raw_image;

    // Default: identity-ish matrix (will be replaced with proper camera matrix)
    vec![
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
    ]
}
