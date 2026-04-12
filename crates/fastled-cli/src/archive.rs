//! Archive download, verification, and extraction utilities.
//!
//! Ports the core logic from `emscripten_archive.py` and `frontend_esbuild.py`:
//! * HTTP(S) download with streaming
//! * SHA-256 file verification
//! * `.tar.zst` extraction (used for the emscripten toolchain)
//! * `.zip` extraction (general-purpose)
//! * Writing the `.emscripten` config file after installation

// This module is library code — not yet wired into the CLI entry point.
// Suppress dead-code lints until a later integration phase.
#![allow(dead_code)]

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

/// Download a file from `url` and write it to `dest`.
///
/// Uses a blocking reqwest client with a 120-second timeout, streaming the
/// response body so large archives do not need to be buffered in memory.
///
/// # Errors
/// Returns an error if the HTTP request fails, the server returns a non-2xx
/// status, or writing the destination file fails.
pub fn download(url: &str, dest: &Path) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("failed to build HTTP client")?;

    let mut response = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url} failed"))?;

    response
        .error_for_status_ref()
        .with_context(|| format!("server returned error for {url}"))?;

    let file = File::create(dest).with_context(|| format!("cannot create {}", dest.display()))?;
    let mut writer = BufWriter::new(file);

    let mut buf = vec![0u8; 1024 * 1024]; // 1 MiB chunks
    loop {
        let n = response
            .read(&mut buf)
            .with_context(|| format!("read error while downloading {url}"))?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .with_context(|| format!("write error while downloading to {}", dest.display()))?;
    }
    writer
        .flush()
        .with_context(|| format!("flush error for {}", dest.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// SHA-256 verification
// ---------------------------------------------------------------------------

/// Compute the SHA-256 hex digest of `path`.
pub fn sha256_file(path: &Path) -> Result<String> {
    let file =
        File::open(path).with_context(|| format!("cannot open {} for hashing", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();

    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .with_context(|| format!("read error hashing {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Return `true` when the SHA-256 digest of `path` matches `expected`.
///
/// `expected` must be a lower-case hex string (64 characters).
///
/// # Errors
/// Returns an error if the file cannot be read.
pub fn verify_sha256(path: &Path, expected: &str) -> Result<bool> {
    let actual = sha256_file(path)?;
    Ok(actual.eq_ignore_ascii_case(expected))
}

// ---------------------------------------------------------------------------
// tar.zst extraction
// ---------------------------------------------------------------------------

/// Extract a `.tar.zst` (zstandard-compressed tar) archive to `dest`.
///
/// If the archive contains a single top-level directory, its contents are
/// promoted one level up (mirrors the Python behaviour in `_extract_archive`).
///
/// # Errors
/// Returns an error if the archive cannot be read or if any entry cannot be
/// written to `dest`.
pub fn extract_tar_zst(archive: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)
        .with_context(|| format!("cannot create destination {}", dest.display()))?;

    let file = File::open(archive)
        .with_context(|| format!("cannot open archive {}", archive.display()))?;
    let buf_reader = BufReader::new(file);

    let zstd_decoder = zstd::stream::read::Decoder::new(buf_reader)
        .context("failed to initialise zstd decoder")?;

    let mut tar_archive = tar::Archive::new(zstd_decoder);
    tar_archive
        .unpack(dest)
        .with_context(|| format!("failed to unpack tar archive to {}", dest.display()))?;

    // Promote single top-level directory (mirrors Python's behaviour).
    _promote_single_child(dest)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// ZIP extraction
// ---------------------------------------------------------------------------

/// Extract a `.zip` archive to `dest`.
///
/// Creates `dest` if it does not exist. All entries (files and directories)
/// are extracted, preserving relative paths.
///
/// # Errors
/// Returns an error if the archive cannot be read or any entry cannot be
/// written.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)
        .with_context(|| format!("cannot create destination {}", dest.display()))?;

    let file = File::open(archive)
        .with_context(|| format!("cannot open zip archive {}", archive.display()))?;
    let buf_reader = BufReader::new(file);
    let mut zip = zip::ZipArchive::new(buf_reader)
        .with_context(|| format!("cannot parse zip archive {}", archive.display()))?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .with_context(|| format!("cannot read zip entry {i}"))?;

        let entry_path: PathBuf = entry
            .enclosed_name()
            .with_context(|| format!("zip entry {i} has an unsafe path"))?
            .to_path_buf();

        let out_path = dest.join(&entry_path);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("cannot create dir {}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("cannot create parent {}", parent.display()))?;
            }
            let mut out_file = File::create(&out_path)
                .with_context(|| format!("cannot create {}", out_path.display()))?;
            std::io::copy(&mut entry, &mut out_file)
                .with_context(|| format!("cannot write {}", out_path.display()))?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// .emscripten config generation
// ---------------------------------------------------------------------------

/// Write the `.emscripten` configuration file into `install_dir`.
///
/// Paths written into the config file reference the **final** `install_dir`
/// (not a staging directory), matching the Python implementation's post-rename
/// behaviour.
///
/// `node_path` is the path to the Node.js executable.  Pass the result of
/// `which("node")` or a literal `"node"` when the executable is on `PATH`.
///
/// # Errors
/// Returns an error if the file cannot be written.
pub fn write_emscripten_config(install_dir: &Path, node_path: &str) -> Result<()> {
    let bin_dir = install_dir.join("bin");
    // Normalise to forward-slash paths (the Python side does the same).
    let llvm_root = path_to_forward_slash(&bin_dir);
    let binaryen_root = path_to_forward_slash(install_dir);

    let config = format!(
        "# Emscripten configuration file\n\
         # Auto-generated by fastled\n\
         \n\
         LLVM_ROOT = {llvm_root:?}\n\
         BINARYEN_ROOT = {binaryen_root:?}\n\
         NODE_JS = {node_path:?}\n\
         "
    );

    let config_path = install_dir.join(".emscripten");
    fs::write(&config_path, config)
        .with_context(|| format!("cannot write {}", config_path.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Convert a path to a string with forward slashes (for cross-platform config
/// files).
fn path_to_forward_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// If `dir` contains exactly one child that is itself a directory, move all of
/// that child's contents up into `dir` and remove the now-empty child.
///
/// This mirrors the Python logic in `_extract_archive`.
fn _promote_single_child(dir: &Path) -> Result<()> {
    let children: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .filter_map(|e| e.ok().map(|de| de.path()))
        .collect();

    if children.len() == 1 && children[0].is_dir() {
        let extracted_root = &children[0];
        let sub_children: Vec<PathBuf> = fs::read_dir(extracted_root)
            .with_context(|| format!("cannot read {}", extracted_root.display()))?
            .filter_map(|e| e.ok().map(|de| de.path()))
            .collect();

        for child in &sub_children {
            let target = dir.join(child.file_name().unwrap());
            fs::rename(child, &target).with_context(|| {
                format!("cannot move {} to {}", child.display(), target.display())
            })?;
        }
        fs::remove_dir_all(extracted_root)
            .with_context(|| format!("cannot remove {}", extracted_root.display()))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // ------------------------------------------------------------------
    // SHA-256 verification
    // ------------------------------------------------------------------

    /// A known SHA-256 digest of the bytes `b"hello fastled"`.
    ///
    /// Computed via: `sha2::Sha256::digest(b"hello fastled")` rendered as hex.
    const HELLO_SHA256: &str = "9371e29d4390b284420993a4161cbda766dc36ce4d8396398a849d1de4d652d6";

    #[test]
    fn test_sha256_known_value() {
        // Use sha2 directly to verify the constant matches the real digest.
        let content = b"hello fastled";
        let mut h = Sha256::new();
        h.update(content);
        let actual = format!("{:x}", h.finalize());
        assert_eq!(actual, HELLO_SHA256, "HELLO_SHA256 constant is incorrect");
    }

    #[test]
    fn test_verify_sha256_correct() {
        let dir = temp_dir();
        let file = dir.path().join("data.bin");
        let content = b"hello fastled";
        fs::write(&file, content).unwrap();

        // Compute expected digest independently.
        let mut h = Sha256::new();
        h.update(content);
        let expected = format!("{:x}", h.finalize());

        let ok = verify_sha256(&file, &expected).expect("verify_sha256");
        assert!(ok, "digest should match");
    }

    #[test]
    fn test_verify_sha256_mismatch() {
        let dir = temp_dir();
        let file = dir.path().join("data.bin");
        fs::write(&file, b"something else").unwrap();

        let ok = verify_sha256(&file, HELLO_SHA256).expect("verify_sha256");
        assert!(!ok, "digest should not match different content");
    }

    #[test]
    fn test_sha256_file_changes_on_content_change() {
        let dir = temp_dir();
        let file = dir.path().join("test.bin");

        fs::write(&file, b"version one").unwrap();
        let h1 = sha256_file(&file).expect("hash1");

        fs::write(&file, b"version two").unwrap();
        let h2 = sha256_file(&file).expect("hash2");

        assert_ne!(h1, h2, "hash must differ after content change");
    }

    // ------------------------------------------------------------------
    // ZIP extraction
    // ------------------------------------------------------------------

    /// Build a minimal in-memory zip containing two files and return the bytes.
    fn make_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);
        let options = zip::write::SimpleFileOptions::default();
        for (name, content) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn test_extract_zip_basic() {
        let dir = temp_dir();
        let zip_path = dir.path().join("test.zip");

        let zip_bytes = make_zip(&[
            ("hello.txt", b"hello world"),
            ("sub/world.txt", b"sub content"),
        ]);
        fs::write(&zip_path, &zip_bytes).unwrap();

        let out = dir.path().join("out");
        extract_zip(&zip_path, &out).expect("extract_zip");

        assert_eq!(fs::read(out.join("hello.txt")).unwrap(), b"hello world");
        assert_eq!(
            fs::read(out.join("sub").join("world.txt")).unwrap(),
            b"sub content"
        );
    }

    #[test]
    fn test_extract_zip_creates_dest() {
        let dir = temp_dir();
        let zip_path = dir.path().join("test.zip");
        let zip_bytes = make_zip(&[("file.txt", b"data")]);
        fs::write(&zip_path, &zip_bytes).unwrap();

        // Destination does not exist yet.
        let out = dir.path().join("nested").join("dest");
        extract_zip(&zip_path, &out).expect("extract_zip");

        assert_eq!(fs::read(out.join("file.txt")).unwrap(), b"data");
    }

    // ------------------------------------------------------------------
    // .emscripten config generation
    // ------------------------------------------------------------------

    #[test]
    fn test_write_emscripten_config_paths_not_staging() {
        let dir = temp_dir();
        let install_dir = dir.path().join("emscripten").join("3.1.50");
        fs::create_dir_all(&install_dir).unwrap();

        write_emscripten_config(&install_dir, "node").expect("write_emscripten_config");

        let config_path = install_dir.join(".emscripten");
        assert!(config_path.exists(), ".emscripten file should exist");

        let contents = fs::read_to_string(&config_path).unwrap();

        // Paths must reference install_dir, not any staging variant.
        assert!(
            !contents.contains("staging"),
            "config must not reference staging: {contents}"
        );

        // LLVM_ROOT must point to the bin subdirectory.
        let bin_dir_str = path_to_forward_slash(&install_dir.join("bin"));
        assert!(
            contents.contains(&bin_dir_str),
            "LLVM_ROOT should contain bin dir ({bin_dir_str}), got:\n{contents}"
        );

        // BINARYEN_ROOT must reference install_dir itself.
        let install_str = path_to_forward_slash(&install_dir);
        assert!(
            contents.contains(&install_str),
            "BINARYEN_ROOT should contain install dir ({install_str}), got:\n{contents}"
        );

        // NODE_JS entry must be present.
        assert!(
            contents.contains("NODE_JS"),
            "config should contain NODE_JS: {contents}"
        );
    }

    #[test]
    fn test_write_emscripten_config_uses_forward_slashes() {
        let dir = temp_dir();
        let install_dir = dir.path().join("emscripten").join("3.1.50");
        fs::create_dir_all(&install_dir).unwrap();

        write_emscripten_config(&install_dir, "/usr/bin/node").expect("write_emscripten_config");

        let config_path = install_dir.join(".emscripten");
        let contents = fs::read_to_string(&config_path).unwrap();

        // All path separators should be forward slashes.
        assert!(
            !contents.contains('\\'),
            "config should not contain backslashes: {contents}"
        );
    }

    #[test]
    fn test_write_emscripten_config_node_path_preserved() {
        let dir = temp_dir();
        let install_dir = dir.path().join("emscripten").join("3.1.50");
        fs::create_dir_all(&install_dir).unwrap();

        let node = "/custom/path/to/node";
        write_emscripten_config(&install_dir, node).expect("write_emscripten_config");

        let contents = fs::read_to_string(install_dir.join(".emscripten")).unwrap();
        assert!(
            contents.contains(node),
            "config should contain the node path ({node}): {contents}"
        );
    }

    // ------------------------------------------------------------------
    // promote single child helper
    // ------------------------------------------------------------------

    #[test]
    fn test_promote_single_child_unwraps_wrapper_dir() {
        let dir = temp_dir();
        let wrapper = dir.path().join("wrapper");
        let inner = wrapper.join("inner");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("file.txt"), b"content").unwrap();

        _promote_single_child(&wrapper).expect("promote");

        // After promotion the file should live directly in wrapper/.
        assert!(wrapper.join("file.txt").exists());
        // The inner/ sub-directory should be gone.
        assert!(!inner.exists());
    }

    #[test]
    fn test_promote_single_child_noop_when_multiple_children() {
        let dir = temp_dir();
        let root = dir.path().join("root");
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();

        _promote_single_child(&root).expect("promote");

        // Both children remain.
        assert!(root.join("a").exists());
        assert!(root.join("b").exists());
    }
}
