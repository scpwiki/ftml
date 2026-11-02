extern crate built;

use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    if let Ok(profile) = env::var("PROFILE") {
        println!("cargo:rustc-cfg=build={:?}", profile);
    }

    built::write_built_file().expect("Failed to compile build information!");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be set"));
    let built_file_path = out_dir.join("built.rs");

    if declares_git_commit_hash(&built_file_path) {
        return;
    }

    let mut built_file = OpenOptions::new()
        .append(true)
        .open(built_file_path)
        .expect("built.rs should exist after built writes it");

    let git_commit_hash = match git_commit_hash() {
        Some(hash) => format!("Some({hash:?})"),
        None => "None".to_string(),
    };

    writeln!(
        built_file,
        "\npub static GIT_COMMIT_HASH: Option<&str> = {git_commit_hash};",
    )
    .expect("GIT_COMMIT_HASH should be appended to built.rs");
}

fn declares_git_commit_hash(built_file_path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(built_file_path) else {
        return false;
    };

    contents.contains("pub static GIT_COMMIT_HASH:")
}

fn git_commit_hash() -> Option<String> {
    let package_override = format!(
        "BUILT_OVERRIDE_{}_GIT_COMMIT_HASH",
        env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "ftml".to_string()),
    );

    println!("cargo:rerun-if-env-changed={package_override}");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=BUILT_OVERRIDE_FTML_GIT_COMMIT_HASH");

    if let Some(hash) = env_commit_hash(&package_override) {
        return Some(hash);
    }

    if let Some(hash) = env_commit_hash("BUILT_OVERRIDE_FTML_GIT_COMMIT_HASH") {
        return Some(hash);
    }

    if let Some(hash) = env_commit_hash("GITHUB_SHA") {
        return Some(hash);
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR")?);
    print_git_rerun_paths(&manifest_dir);

    let output = Command::new("git")
        .arg("-C")
        .arg(&manifest_dir)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let hash = String::from_utf8(output.stdout).ok()?;
    let hash = hash.trim().to_owned();
    is_full_commit_hash(&hash).then_some(hash)
}

fn is_full_commit_hash(hash: &str) -> bool {
    hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn env_commit_hash(variable: &str) -> Option<String> {
    env::var(variable)
        .ok()
        .filter(|hash| is_full_commit_hash(hash))
}

fn print_git_rerun_paths(manifest_dir: &Path) {
    let Some(git_dir) = git_dir(manifest_dir) else {
        return;
    };

    let head_path = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join("packed-refs").display()
    );

    let Ok(head) = fs::read_to_string(head_path) else {
        return;
    };

    if let Some(reference) = head.trim().strip_prefix("ref: ") {
        println!(
            "cargo:rerun-if-changed={}",
            git_dir.join(reference).display()
        );
    }
}

fn git_dir(manifest_dir: &Path) -> Option<PathBuf> {
    let dot_git = manifest_dir.join(".git");

    if dot_git.is_dir() {
        return Some(dot_git);
    }

    let git_file = fs::read_to_string(&dot_git).ok()?;
    let git_dir = git_file.trim().strip_prefix("gitdir: ")?.trim();
    let git_dir = PathBuf::from(git_dir);

    if git_dir.is_absolute() {
        Some(git_dir)
    } else {
        Some(manifest_dir.join(git_dir))
    }
}
