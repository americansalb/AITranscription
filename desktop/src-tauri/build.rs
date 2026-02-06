use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Create a placeholder sidecar binary so tauri_build::build() doesn't fail.
    // When building the vaak-mcp binary target, tauri-build checks that all
    // externalBin entries exist in the binaries/ directory. But vaak-mcp IS the
    // binary we're building, so it doesn't exist yet — chicken-and-egg problem.
    // We create an empty placeholder here; the real binary is copied into place
    // by scripts/copy-sidecar.js after cargo finishes.
    let target = env::var("TARGET").unwrap_or_else(|_| {
        // Fallback: build a triple from std::env::consts
        let arch = env::consts::ARCH;
        let os = env::consts::OS;
        match (arch, os) {
            ("x86_64", "windows") => "x86_64-pc-windows-msvc".to_string(),
            ("x86_64", "linux") => "x86_64-unknown-linux-gnu".to_string(),
            ("x86_64", "macos") => "x86_64-apple-darwin".to_string(),
            ("aarch64", "macos") => "aarch64-apple-darwin".to_string(),
            _ => format!("{}-unknown-{}", arch, os),
        }
    });

    let ext = if target.contains("windows") { ".exe" } else { "" };
    let sidecar_name = format!("vaak-mcp-{}{}", target, ext);

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let binaries_dir = manifest_dir.join("binaries");
    let sidecar_path = binaries_dir.join(&sidecar_name);

    if !sidecar_path.exists() {
        fs::create_dir_all(&binaries_dir).ok();
        // Write a minimal placeholder (empty file or tiny stub)
        if ext == ".exe" {
            // Windows needs a valid PE or at least a non-empty file
            fs::write(&sidecar_path, b"placeholder").ok();
        } else {
            // Unix: create an empty file with a shebang so it's nominally executable
            fs::write(&sidecar_path, b"#!/bin/sh\nexit 1\n").ok();
        }
        println!("cargo:warning=Created placeholder sidecar at {}", sidecar_path.display());
    }

    tauri_build::build()
}
