//! Embed every `factory/presets/*.json` (sorted by file name).
use std::io::Write;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../factory/presets");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("factory/presets")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("factory.rs");
    let mut f = std::fs::File::create(out).unwrap();
    writeln!(f, "pub const FACTORY: &[(&str, &str)] = &[").unwrap();
    for p in files {
        println!("cargo:rerun-if-changed={}", p.display());
        let name = p.file_stem().unwrap().to_string_lossy();
        writeln!(f, "    ({name:?}, include_str!({:?})),", p.canonicalize().unwrap().display().to_string()).unwrap();
    }
    writeln!(f, "];").unwrap();
}
