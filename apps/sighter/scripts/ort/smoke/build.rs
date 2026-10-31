//! Adds what the merged ONNX Runtime archive needs besides itself, from the archive's `link.txt` (written by
//! build-ort.sh): system libraries and frameworks, and on Windows the DLLs to delay-load, so that a missing or broken
//! `DirectML.dll` degrades to the CPU instead of stopping the start (ADR 0001 decision 4). sighter-infer's build script
//! will do the same. Without ORT_LIB_PATH nothing is added and `cargo check` still works.
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=ORT_LIB_PATH");
    let Some(lib) = std::env::var_os("ORT_LIB_PATH").map(PathBuf::from) else {
        return;
    };
    let link = lib.parent().map(|p| p.join("link.txt")).unwrap_or_default();
    println!("cargo:rerun-if-changed={}", link.display());
    let Ok(text) = std::fs::read_to_string(&link) else {
        println!("cargo:warning=no link.txt next to {}: system libraries not added", lib.display());
        return;
    };
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match line.split_once('=') {
            Some(("framework", name)) => println!("cargo:rustc-link-lib=framework={name}"),
            Some(("dylib", name)) => println!("cargo:rustc-link-lib=dylib={name}"),
            Some(("delayload", dll)) => println!("cargo:rustc-link-arg=/DELAYLOAD:{dll}"),
            _ => println!("cargo:warning=link.txt: ignored line '{line}'"),
        }
    }
}
