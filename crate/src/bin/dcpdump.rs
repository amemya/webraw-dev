/// Test binary: parse a DCP file and dump its contents
/// Run: cargo run --bin dcpdump -- <file.dcp>

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: dcpdump <file.dcp>");
        std::process::exit(1);
    }

    let data = std::fs::read(&args[1]).expect("Failed to read file");
    let profile = raw_processor::dcp::parse_dcp(&data).expect("Failed to parse DCP");

    println!("=== DCP Profile ===");
    println!("Name: {}", profile.name);
    println!("Illuminant1: {:?} ({:.0}K)", profile.illuminant1, profile.illuminant1.temperature());
    println!("Illuminant2: {:?} ({:.0}K)", profile.illuminant2, profile.illuminant2.temperature());

    if let Some(ref m) = profile.color_matrix_1 {
        println!("\nColorMatrix1:");
        for r in 0..3 {
            println!("  [{:.6}, {:.6}, {:.6}]", m[r*3], m[r*3+1], m[r*3+2]);
        }
    }
    if let Some(ref m) = profile.color_matrix_2 {
        println!("\nColorMatrix2:");
        for r in 0..3 {
            println!("  [{:.6}, {:.6}, {:.6}]", m[r*3], m[r*3+1], m[r*3+2]);
        }
    }
    if let Some(ref m) = profile.forward_matrix_1 {
        println!("\nForwardMatrix1 (cam → XYZ, illuminant1):");
        for r in 0..3 {
            println!("  [{:.6}, {:.6}, {:.6}]", m[r*3], m[r*3+1], m[r*3+2]);
        }
    }
    if let Some(ref m) = profile.forward_matrix_2 {
        println!("\nForwardMatrix2 (cam → XYZ, illuminant2):");
        for r in 0..3 {
            println!("  [{:.6}, {:.6}, {:.6}]", m[r*3], m[r*3+1], m[r*3+2]);
        }
    }

    if let Some(ref dims) = profile.hue_sat_map_dims {
        println!("\nHueSatMap Dims: {}h × {}s × {}v = {} entries",
            dims.hue_divs, dims.sat_divs, dims.val_divs,
            dims.hue_divs * dims.sat_divs * dims.val_divs);
        println!("HueSatMap Encoding: {}", if profile.hue_sat_map_encoding == 0 { "Linear" } else { "sRGB" });
    }
    if let Some(ref data) = profile.hue_sat_map_data_1 {
        println!("HueSatMapData1: {} entries", data.len());
        // Show first few
        for (i, e) in data.iter().take(5).enumerate() {
            println!("  [{}] hue={:.4}, sat={:.4}, val={:.4}", i, e.hue_shift, e.sat_scale, e.val_scale);
        }
    }
    if let Some(ref data) = profile.hue_sat_map_data_2 {
        println!("HueSatMapData2: {} entries", data.len());
    }

    if let Some(ref dims) = profile.look_table_dims {
        println!("\nLookTable Dims: {}h × {}s × {}v = {} entries",
            dims.hue_divs, dims.sat_divs, dims.val_divs,
            dims.hue_divs * dims.sat_divs * dims.val_divs);
    }
    if let Some(ref data) = profile.look_table_data {
        println!("LookTableData: {} entries", data.len());
    }

    if let Some(ref curve) = profile.tone_curve {
        println!("\nToneCurve: {} points", curve.len());
        // Show first few and last
        for (i, (x, y)) in curve.iter().take(5).enumerate() {
            println!("  [{}] {:.4} → {:.4}", i, x, y);
        }
        if curve.len() > 5 {
            println!("  ...");
            let last = curve.last().unwrap();
            println!("  [{}] {:.4} → {:.4}", curve.len()-1, last.0, last.1);
        }
    }
}
