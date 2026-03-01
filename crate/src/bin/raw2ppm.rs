/// Test binary: decode CR2 and output PPM images for visual verification.
/// Run: cargo run --bin raw2ppm -- <input.cr2> <output.ppm>

use std::fs;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: raw2ppm <input.cr2> <output.ppm>");
        std::process::exit(1);
    }

    let data = fs::read(&args[1]).expect("Failed to read file");

    // === Full pipeline (via our decode module) ===
    let result = raw_processor::decode::decode_raw_bytes(&data).expect("Failed to decode");

    let w = result.metadata.width;
    let h = result.metadata.height;
    let pixels = &result.pixels;

    println!("Decoded: {}x{}", w, h);
    println!("WB coeffs: {:?}", result.metadata.wb_coeffs);
    println!("Color matrix (cam_to_srgb): {:?}", result.metadata.color_matrix);
    println!("xyz_to_cam_raw: {:?}", result.metadata.xyz_to_cam_raw);
    println!("Display-referred (DCP): {}", result.display_referred);

    // Sample center pixels
    let cx = w / 2;
    let cy = h / 2;
    for dy in 0..3 {
        let idx = ((cy + dy) * w + cx) * 3;
        println!("  pixel({},{}) R={:.4} G={:.4} B={:.4}", cx, cy+dy,
            pixels[idx], pixels[idx+1], pixels[idx+2]);
    }

    // Write PPM — apply sRGB gamma only if data is linear (non-DCP path)
    write_ppm(&args[2], pixels, w, h, result.display_referred);
    println!("Written: {}", args[2]);

    // === Minimal pipeline: just normalize + WB + demosaic, NO color matrix ===
    let (rgb_no_mat, w2, h2) = decode_minimal(&data);
    let no_mat_path = args[2].replace(".ppm", "_no_matrix.ppm");
    write_ppm(&no_mat_path, &rgb_no_mat, w2, h2, false);
    println!("Written (no matrix): {}", no_mat_path);
}

fn write_ppm(path: &str, pixels: &[f32], w: usize, h: usize, display_referred: bool) {
    let mut ppm = Vec::with_capacity(w * h * 3 + 100);
    write!(ppm, "P6\n{} {}\n255\n", w, h).unwrap();
    for i in 0..(w * h) {
        let idx = i * 3;
        if display_referred {
            // DCP output: already display-referred, just scale to 0-255
            ppm.push(to_u8(pixels[idx]));
            ppm.push(to_u8(pixels[idx + 1]));
            ppm.push(to_u8(pixels[idx + 2]));
        } else {
            // Linear data: apply sRGB gamma
            ppm.push(to_u8(linear_to_srgb(pixels[idx])));
            ppm.push(to_u8(linear_to_srgb(pixels[idx + 1])));
            ppm.push(to_u8(linear_to_srgb(pixels[idx + 2])));
        }
    }
    fs::write(path, &ppm).expect("Failed to write PPM");
}

/// Minimal decode: normalize + WB + demosaic, NO color matrix
fn decode_minimal(data: &[u8]) -> (Vec<f32>, usize, usize) {
    let mut cursor = std::io::Cursor::new(data);
    let img = rawloader::decode(&mut cursor).expect("decode");

    let fw = img.width;
    let ct = img.crops[0];
    let cr = img.crops[1];
    let cb = img.crops[2];
    let cl = img.crops[3];
    let w = fw - cl - cr;
    let h = img.height - ct - cb;

    let wb_r = img.wb_coeffs[0] / img.wb_coeffs[1];
    let wb_b = img.wb_coeffs[2] / img.wb_coeffs[1];

    let max_bl = img.blacklevels.iter().cloned().fold(0u16, |a, b| a.max(b as u16));

    let raw_data = match &img.data {
        rawloader::RawImageData::Integer(d) => d,
        _ => panic!("Float not supported"),
    };

    let mut normalized = Vec::with_capacity(w * h);
    for row in 0..h {
        for col in 0..w {
            let fr = row + ct;
            let fc = col + cl;
            let idx = fr * fw + fc;
            let color = img.cfa.color_at(fr, fc);
            let cfa_idx = (fr % 2) * 2 + (fc % 2);
            let bl = {
                let v = img.blacklevels[cfa_idx] as u16;
                if v == 0 { max_bl } else { v }
            };
            let wl = img.whitelevels[cfa_idx];
            let raw_val = raw_data[idx] as f32;
            let norm = ((raw_val - bl as f32) / (wl as f32 - bl as f32)).max(0.0);
            let wb = match color { 0 => wb_r, 1 => 1.0, 2 => wb_b, _ => 1.0 };
            normalized.push(norm * wb);
        }
    }

    // Parse CFA for cropped area
    let mut cfa_pattern = [1u8; 4];
    for r in 0..2 {
        for c in 0..2 {
            cfa_pattern[r * 2 + c] = img.cfa.color_at(r + ct, c + cl) as u8;
        }
    }

    let rgb = raw_processor::demosaic::demosaic_bilinear(&normalized, w, h, &cfa_pattern);
    (rgb, w, h)
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 }
    else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn to_u8(v: f32) -> u8 {
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}
