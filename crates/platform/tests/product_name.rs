//! The product name is data, not code. These tests keep it that way, so the
//! working name can change without touching a line of code.

use std::fs;
use std::path::{Path, PathBuf};

/// Files that may contain the name: the name file itself and the readme.
const ALLOWED: &[&str] = &["PRODUCT_NAME", "README.md"];
/// Never source: git metadata (a directory, or a pointer file in a worktree)
/// and build output.
const SKIP: &[&str] = &[".git", "target", "node_modules"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root exists")
}

/// Lowercase alphanumeric words, so `Name`, `name-platform`, and `name_thing`
/// all count as the name while a longer unrelated word does not.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

fn contains(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("directory is readable") {
        let path = entry.expect("entry is readable").path();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if SKIP.contains(&file_name.as_str()) {
            continue;
        }
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn the_name_is_one_trimmed_non_empty_line() {
    let raw = fs::read_to_string(repo_root().join("PRODUCT_NAME")).expect("PRODUCT_NAME exists");
    let name = platform::PRODUCT_NAME;
    assert!(!name.is_empty(), "PRODUCT_NAME is empty");
    assert_eq!(name, raw.trim(), "PRODUCT_NAME is read exactly as written, minus surrounding whitespace");
    assert!(!name.contains('\n'), "PRODUCT_NAME must be a single line");
}

#[test]
fn the_name_appears_nowhere_but_the_name_file_and_the_readme() {
    let root = repo_root();
    let name = words(platform::PRODUCT_NAME);
    let mut files = Vec::new();
    walk(&root, &mut files);

    let mut leaks = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .expect("file is under the repository root")
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWED.contains(&relative.as_str()) {
            continue;
        }
        // Binary files are not text and cannot name anything.
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if contains(&words(&text), &name) {
            leaks.push(relative);
        }
    }

    assert!(
        leaks.is_empty(),
        "the product name is written outside PRODUCT_NAME and README.md, so a rename would touch code: {:?}",
        leaks
    );
}
