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

    // Invalidate cargo's incremental cache when dist/ contents change. Without
    // this, a TS/CSS-only edit + `npm run build` updates `desktop/dist/` on
    // disk but cargo's no-source-changed check short-circuits — meaning
    // `cargo build` finishes in <1s without re-linking, leaving the prior
    // binary in place with the OLD embedded dist. That bit the team during
    // the 2026-05-22 v1.3 activation cycle: cargo said "Finished in 1s"
    // and the human launched the same stale binary repeatedly. Per evil-
    // arch msg 297 + dev-challenger flag #2 of msg 379 — Instance #16 in
    // .vaak/design-notes/multi-writer-audit-2026-05-13.md.
    //
    // The `../dist` path is relative to CARGO_MANIFEST_DIR (`desktop/src-
    // tauri/`), resolving to `desktop/dist/`. Cargo treats directory paths
    // as "rerun if any descendant file changes."
    println!("cargo:rerun-if-changed=../dist");

    // SHA-5.3c board-path lint. Closes the class of bug found by
    // dev-challenger msg 1202 (launcher.rs:835) and tester msg 1209
    // (collab.rs:6118) — both hardcoded `.vaak/board.jsonl` instead of
    // routing through `crate::collab::active_board_path()`. On a non-
    // default section the broadcast lands in legacy root and section-
    // active agents never see it (silent dead-floor variant).
    //
    // Per evil-arch msg 1212: trust-summary-over-diff-read audits failed
    // twice in 30 minutes (SHA-5.1 review + architect msg 1189). Build-
    // time grep is the only audit that can't be bypassed by review
    // shortcut. Pre-commit hooks fail under `--no-verify` pressure;
    // cargo-test fires only when tests run; build.rs fires on every
    // compile and cannot be skipped without editing the lint itself.
    lint_no_legacy_board_path(&manifest_dir.join("src"));

    tauri_build::build()
}

/// Scan `src/**/*.rs` for hardcoded references to the legacy root
/// `.vaak/board.jsonl` path. Any production-code occurrence must use
/// `crate::collab::active_board_path(dir)` / `super::active_board_path(dir)`
/// / `board_jsonl_path(dir)` instead. Lines may opt out with an inline
/// `// LINT_EXEMPT_BOARD_PATH: <reason>` comment for the four legitimate
/// categories: resolver internals, init writes, aggregation reads, test
/// code (per tester:0 msg 1221 categorization).
///
/// Patterns detected (per evil-arch msg 1226 / tester msg 1228 n3 ask):
///   a) literal: `.vaak/board.jsonl` (slash form)
///   b) split same-line: `.join(".vaak").join("board.jsonl")`
///   c) split multi-line: `.join(".vaak")\n    .join("board.jsonl")`
///
/// NOT detected (acknowledged limitations — fall back to PR review):
///   - string constants: `const BOARD: &str = "board.jsonl"; path.join(BOARD)`
///   - whitespace-inside-parens: `.join( ".vaak" )` (rustfmt collapses these)
///   - macro-generated paths
fn lint_no_legacy_board_path(src_dir: &PathBuf) {
    let mut violations: Vec<String> = Vec::new();
    let mut watched_files: Vec<PathBuf> = Vec::new();
    walk_rs_files(src_dir, &mut |path: &std::path::Path| {
        watched_files.push(path.to_path_buf());
        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return,
        };
        // Pass 1: line-by-line scan for single-line forms (literal + same-line split).
        // Strips trailing `//` comments to skip docstrings.
        for (idx, line) in contents.lines().enumerate() {
            if line.contains("LINT_EXEMPT_BOARD_PATH") {
                continue;
            }
            let code = match line.find("//") {
                Some(i) => &line[..i],
                None => line,
            };
            let has_literal = code.contains(".vaak/board.jsonl");
            let has_split_same_line = code.contains(".join(\".vaak\").join(\"board.jsonl\")");
            if has_literal || has_split_same_line {
                violations.push(format!(
                    "  {}:{}: hardcoded legacy board path",
                    path.display(),
                    idx + 1
                ));
            }
        }
        // Pass 2: full-file lookahead for split multi-line forms.
        // For each `.join(".vaak")` occurrence, look ahead through whitespace
        // (including newlines) for `.join("board.jsonl")`. If the literal
        // appears within the lookahead window AND the originating line lacks
        // LINT_EXEMPT_BOARD_PATH, flag it. The line-by-line pass already
        // caught the same-line case; the lookahead specifically catches the
        // multi-line variant.
        let vaak_marker = ".join(\".vaak\")";
        let board_marker = ".join(\"board.jsonl\")";
        let mut search_start = 0;
        while let Some(rel) = contents[search_start..].find(vaak_marker) {
            let abs = search_start + rel;
            // Resolve line number + line content for exemption + reporting
            let line_start = contents[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_end = contents[abs..]
                .find('\n')
                .map(|p| abs + p)
                .unwrap_or(contents.len());
            let originating_line = &contents[line_start..line_end];
            let originating_line_num = contents[..abs].matches('\n').count() + 1;
            // Skip if this occurrence is on a line caught by pass 1 already
            // (same-line .join(".vaak").join("board.jsonl") — would be a
            // duplicate report) OR is in an exempt line.
            let already_caught_same_line = originating_line.contains(
                ".join(\".vaak\").join(\"board.jsonl\")",
            );
            let is_exempt = originating_line.contains("LINT_EXEMPT_BOARD_PATH");
            if !already_caught_same_line && !is_exempt {
                // Look ahead through whitespace+newlines for board_marker.
                // Cap lookahead at 256 chars to bound work.
                let after = abs + vaak_marker.len();
                let cap = (after + 256).min(contents.len());
                let lookahead = &contents[after..cap];
                let trimmed = lookahead.trim_start();
                if trimmed.starts_with(board_marker) {
                    violations.push(format!(
                        "  {}:{}: hardcoded legacy board path (multi-line split)",
                        path.display(),
                        originating_line_num
                    ));
                }
            }
            search_start = abs + vaak_marker.len();
        }
    });
    // Cargo rerun-if-changed for each .rs source so the lint re-fires
    // on any source edit (catches reintroduction by future commits).
    for f in &watched_files {
        println!("cargo:rerun-if-changed={}", f.display());
    }
    if !violations.is_empty() {
        eprintln!("\n\nSHA-5.3c board-path lint FAILED ({} violation(s)):\n", violations.len());
        for v in &violations {
            eprintln!("{}", v);
        }
        eprintln!(
            "\nUse a section-aware resolver for any board-write path:\n  \
              - crate::collab::active_board_path(dir)\n  \
              - super::active_board_path(dir)        (inside collab module)\n  \
              - board_jsonl_path(dir)                (inside vaak-mcp)\n\n\
            For legitimate exceptions (resolver internals, init writes, aggregation reads,\n\
            test code), add an inline `// LINT_EXEMPT_BOARD_PATH: <category>` comment.\n"
        );
        panic!("SHA-5.3c lint: {} hardcoded board.jsonl path(s)", violations.len());
    }
}

fn walk_rs_files(dir: &std::path::Path, cb: &mut dyn FnMut(&std::path::Path)) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk_rs_files(&p, cb);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                cb(&p);
            }
        }
    }
}
