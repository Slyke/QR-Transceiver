use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).expect("read source directory") {
            collect(&entry.expect("source entry").path(), files);
        }
    } else if path.is_file() {
        files.push(path.to_path_buf());
    }
}
fn main() {
    println!("cargo:rerun-if-env-changed=BUILD_HASH");
    let root = Path::new("../..");
    let mut files = Vec::new();
    for path in [
        "src",
        "crates",
        "cli",
        "src-tauri/src",
        "src-tauri/capabilities",
        "Cargo.toml",
        "Cargo.lock",
        "package.json",
        "package-lock.json",
        "rust-toolchain.toml",
        "src-tauri/Cargo.toml",
        "src-tauri/tauri.conf.json",
    ] {
        collect(&root.join(path), &mut files);
    }
    files.sort();
    let mut digest = Sha256::new();
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .strip_prefix(root)
            .expect("source path")
            .to_string_lossy()
            .replace('\\', "/");
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(std::fs::read(path).expect("read build input"));
    }
    let source_hash = format!("{:x}", digest.finalize());
    let hash = std::env::var("BUILD_HASH").unwrap_or_else(|_| source_hash[..16].to_string());
    println!("cargo:rustc-env=BUILD_HASH={hash}");
}
