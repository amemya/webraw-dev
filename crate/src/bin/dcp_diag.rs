/// Diagnostic: test DCP pipeline components individually
/// Outputs multiple PPM files, each disabling different components

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: dcp_diag <input.cr2>");
        std::process::exit(1);
    }

    let data = std::fs::read(&args[1]).expect("Failed to read file");
    let dcp_data = include_bytes!("../../../profiles/Canon EOS-1D X Camera Standard.dcp");
    let mut profile = raw_processor::dcp::parse_dcp(dcp_data).expect("Failed to parse DCP");

    // Show some LookTable entries at different hue positions
    if let Some(ref data) = profile.look_table_data {
        if let Some(ref dims) = profile.look_table_dims {
            let sd = dims.sat_divs as usize;
            let vd = dims.val_divs as usize;
            println!("=== LookTable samples (90h x 16s x 16v) ===");
            for hi in [0, 15, 30, 45, 60, 75, 89] {
                // At medium saturation (s=8) and medium value (v=8)
                let si = 8;
                let vi = 8;
                let idx = (hi * sd + si) * vd + vi;
                let e = &data[idx];
                println!("  hue={} ({}°), sat={}, val={}: h_shift={:.4}, s_scale={:.4}, v_scale={:.4}",
                    hi, hi * 4, si, vi, e.hue_shift, e.sat_scale, e.val_scale);
            }
        }
    }

    // Show tone curve shape
    if let Some(ref curve) = profile.tone_curve {
        println!("\n=== Tone curve at key points ===");
        for &input in &[0.0, 0.01, 0.02, 0.05, 0.1, 0.18, 0.25, 0.5, 0.75, 1.0] {
            // Find the output
            let mut output = input;
            for j in 0..curve.len() - 1 {
                if curve[j].0 <= input && curve[j + 1].0 >= input {
                    let x0 = curve[j].0;
                    let y0 = curve[j].1;
                    let x1 = curve[j + 1].0;
                    let y1 = curve[j + 1].1;
                    output = if (x1 - x0).abs() < 1e-10 { y0 } else { y0 + (y1 - y0) * (input - x0) / (x1 - x0) };
                    break;
                }
            }
            println!("  {:.3} → {:.3} (ratio: {:.2}x)", input, output,
                if input > 0.001 { output / input } else { 0.0 });
        }
    }

    // Decode the image with the full pipeline
    let result = raw_processor::decode::decode_raw_bytes(&data).expect("decode");
    let w = result.metadata.width;
    let h = result.metadata.height;

    // Sample different areas
    println!("\n=== Pixel values after full DCP pipeline ===");
    // Top-center (likely sky/bokeh)
    sample_area(&result.pixels, w, h, w/2, h/4, "Top-center (bokeh)");
    // Center (near flowers)
    sample_area(&result.pixels, w, h, w/2, h/2, "Center");
    // Where white petals might be (~40% from left, ~55% from top based on image)
    sample_area(&result.pixels, w, h, (w*2)/5, (h*55)/100, "White petal area");
    // Where green leaves might be (~35% from left, ~75% from top)
    sample_area(&result.pixels, w, h, (w*35)/100, (h*75)/100, "Green leaf area");
}

fn sample_area(pixels: &[f32], w: usize, h: usize, cx: usize, cy: usize, label: &str) {
    let mut r_sum = 0.0f64;
    let mut g_sum = 0.0f64;
    let mut b_sum = 0.0f64;
    let mut count = 0;
    for dy in 0..10 {
        for dx in 0..10 {
            let x = (cx + dx).min(w - 1);
            let y = (cy + dy).min(h - 1);
            let idx = (y * w + x) * 3;
            r_sum += pixels[idx] as f64;
            g_sum += pixels[idx + 1] as f64;
            b_sum += pixels[idx + 2] as f64;
            count += 1;
        }
    }
    let r = r_sum / count as f64;
    let g = g_sum / count as f64;
    let b = b_sum / count as f64;
    // Also show after sRGB gamma
    let rs = linear_to_srgb(r);
    let gs = linear_to_srgb(g);
    let bs = linear_to_srgb(b);
    println!("  {} ({},{}): linear R={:.4} G={:.4} B={:.4}, sRGB R={:.3} G={:.3} B={:.3}",
        label, cx, cy, r, g, b, rs, gs, bs);
}

fn linear_to_srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 }
    else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}
