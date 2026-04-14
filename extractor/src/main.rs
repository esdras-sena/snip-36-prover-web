use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use apollo_starknet_os_program::VIRTUAL_OS_PROGRAM_BYTES;

fn main() {
    let args: Vec<String> = env::args().collect();

    let output_path = match args.get(1) {
        Some(path) => PathBuf::from(path),
        None => {
            eprintln!("Usage: virtual-os-extractor <output-path>");
            eprintln!("  Extracts VIRTUAL_OS_PROGRAM_BYTES to a JSON file.");
            process::exit(1);
        }
    };

    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("Error: failed to create output directory: {e}");
                process::exit(1);
            });
        }
    }

    fs::write(&output_path, VIRTUAL_OS_PROGRAM_BYTES).unwrap_or_else(|e| {
        eprintln!("Error: failed to write output file: {e}");
        process::exit(1);
    });

    println!("Virtual OS program written to: {}", output_path.display());
    println!("Size: {} bytes", VIRTUAL_OS_PROGRAM_BYTES.len());
}
