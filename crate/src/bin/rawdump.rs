/// Test binary to dump rawloader metadata from a CR2 file
/// Run: cargo run --bin rawdump -- <path_to_file.cr2>

use std::fs;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: rawdump <file.cr2>");
        std::process::exit(1);
    }

    let data = fs::read(&args[1]).expect("Failed to read file");
    let mut cursor = std::io::Cursor::new(&data);
    let img = rawloader::decode(&mut cursor).expect("Failed to decode");

    println!("=== RAW Image Metadata ===");
    println!("Make: {}", img.make);
    println!("Model: {}", img.model);
    println!("Dimensions: {}x{}", img.width, img.height);
    println!("CPP: {}", img.cpp);
    println!("Crops [top, right, bottom, left]: {:?}", img.crops);
    println!("WB coeffs: {:?}", img.wb_coeffs);
    println!("Black levels: {:?}", img.blacklevels);
    println!("White levels: {:?}", img.whitelevels);
    println!("");

    println!("=== CFA Pattern ===");
    for row in 0..4 {
        for col in 0..4 {
            let c = img.cfa.color_at(row, col);
            let ch = match c { 0 => 'R', 1 => 'G', 2 => 'B', _ => '?' };
            print!("{}", ch);
        }
        println!("");
    }
    println!("");

    // With crop offset
    let ct = img.crops[0];
    let cl = img.crops[3];
    println!("=== CFA Pattern (with crop offset top={}, left={}) ===", ct, cl);
    for row in 0..4 {
        for col in 0..4 {
            let c = img.cfa.color_at(row + ct, col + cl);
            let ch = match c { 0 => 'R', 1 => 'G', 2 => 'B', _ => '?' };
            print!("{}", ch);
        }
        println!("");
    }
    println!("");

    println!("=== xyz_to_cam ===");
    for r in 0..4 {
        println!("  Row {}: {:?}", r, img.xyz_to_cam[r]);
    }
    println!("");

    // Show some raw pixel values
    println!("=== Raw pixel values (first 20 of active area) ===");
    match &img.data {
        rawloader::RawImageData::Integer(data) => {
            let w = img.width;
            println!("Data type: Integer (u16), total pixels: {}", data.len());
            // Show first few pixels of the CROPPED area
            for row in 0..2 {
                for col in 0..10 {
                    let fr = row + ct;
                    let fc = col + cl;
                    let idx = fr * w + fc;
                    let c = img.cfa.color_at(fr, fc);
                    let ch = match c { 0 => 'R', 1 => 'G', 2 => 'B', _ => '?' };
                    print!("  ({},{})={}: {}  ", row, col, ch, data[idx]);
                }
                println!("");
            }
            println!("");

            // Show pixel value ranges per channel for the center of the image
            let cx = img.width / 2;
            let cy = img.height / 2;
            println!("=== Center 100x100 pixel stats ===");
            let mut r_vals = Vec::new();
            let mut g_vals = Vec::new();
            let mut b_vals = Vec::new();
            for row in cy..cy+100 {
                for col in cx..cx+100 {
                    let idx = row * w + col;
                    let c = img.cfa.color_at(row, col);
                    let val = data[idx];
                    match c {
                        0 => r_vals.push(val),
                        1 => g_vals.push(val),
                        2 => b_vals.push(val),
                        _ => {}
                    }
                }
            }
            let stats = |name: &str, vals: &mut Vec<u16>| {
                if vals.is_empty() { return; }
                vals.sort();
                let min = vals[0];
                let max = vals[vals.len()-1];
                let median = vals[vals.len()/2];
                let mean: f64 = vals.iter().map(|&x| x as f64).sum::<f64>() / vals.len() as f64;
                println!("  {} (n={}): min={}, max={}, median={}, mean={:.1}", name, vals.len(), min, max, median, mean);
            };
            stats("R", &mut r_vals);
            stats("G", &mut g_vals);
            stats("B", &mut b_vals);
        }
        rawloader::RawImageData::Float(data) => {
            println!("Data type: Float (f32), total pixels: {}", data.len());
        }
    }
}
