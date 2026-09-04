//! Example: use backtrip as a library to decompile a .NET assembly to C#.
//!
//! ```bash
//! cargo run --example decompile -- path/to/Assembly.dll
//! ```
//!
//! The example parses the PE/CLI metadata, decompiles every type to C#,
//! prints a summary of the decompiled types, then previews the first one.

use std::path::PathBuf;

use backtrip::decompile::decompile_assembly;
use backtrip::metadata::{load, Reader};
use backtrip::pe::PeImage;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: decompile <assembly.dll>");
    let data = std::fs::read(&path).expect("failed to read assembly");
    let pe = PeImage::parse(data).expect("invalid PE (not a managed .NET assembly?)");
    let (root, tables) = load(&pe).expect("failed to parse metadata streams");
    let reader = Reader::new(&pe, &root, &tables).expect("failed to build reader");

    let types = decompile_assembly(&reader).expect("decompilation failed");
    let file_name = PathBuf::from(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    println!("Decompiled {} type(s) from {file_name}:", types.len());
    for t in &types {
        println!("  - {}", t.file_name);
    }

    if let Some(first) = types.first() {
        println!("\nPreview of {}:\n", first.file_name);
        for line in first.source.lines().take(12) {
            println!("{line}");
        }
    }
}
