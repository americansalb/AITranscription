use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn emit_git_build_info() {
    let sha = Command::new("git").args(["rev-parse", "HEAD"]).output()
        .ok().and_then(|o| if o.status.success() { String::from_utf8(o.stdout).ok() } else { None })
        .map(|s| s.trim().to_string()).unwrap_or_else(|| "unknown".into());
    let dirty = Command::new("git").args(["diff", "--quiet", "HEAD"]).status()
        .map(|s| !s.success()).unwrap_or(false);
    let subject_raw = Command::new("git").args(["log", "-1", "--format=%s"]).output()
        .ok().and_then(|o| if o.status.success() { String::from_utf8(o.stdout).ok() } else { None })
        .map(|s| s.trim().to_string()).unwrap_or_default();
    let subject: String = subject_raw.chars().take(80).collect();
    let commit_date = Command::new("git").env("TZ", "UTC0")
        .args(["log", "-1", "--date=format-local:%Y-%m-%dT%H:%M:%SZ", "--format=%cd"]).output()
        .ok().and_then(|o| if o.status.success() { String::from_utf8(o.stdout).ok() } else { None })
        .map(|s| s.trim().to_string()).unwrap_or_default();
    let tag = Command::new("git").args(["describe", "--tags", "--exact-match"]).output()
        .ok().and_then(|o| if o.status.success() { String::from_utf8(o.stdout).ok() } else { None })
        .map(|s| s.trim().to_string()).unwrap_or_default();
    let built_at = chrono_like_iso_now();
    println!("cargo:rustc-env=VAAK_GIT_SHA={}", sha);
    println!("cargo:rustc-env=VAAK_GIT_DIRTY={}", dirty);
    println!("cargo:rustc-env=VAAK_GIT_SUBJECT={}", subject);
    println!("cargo:rustc-env=VAAK_GIT_COMMIT_DATE={}", commit_date);
    println!("cargo:rustc-env=VAAK_GIT_TAG={}", tag);
    println!("cargo:rustc-env=VAAK_BUILT_AT={}", built_at);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}

fn chrono_like_iso_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("unix:{}", now)
}

fn main() {
    emit_git_build_info();
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
