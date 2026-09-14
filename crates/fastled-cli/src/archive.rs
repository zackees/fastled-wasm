//! Archive download, verification, and extraction utilities.
//!
//! * HTTP(S) download with streaming
//! * SHA-256 file verification
//! * `.tar.zst` extraction (used for the emscripten toolchain)
//! * `.zip` extraction (general-purpose)
//! * Writing the `.emscripten` config file after installation

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::error_compat::{Context, Result};
use kernal_api::archive::{ArchiveFormat, DanglingLinks, ExtractionLimits};

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

/// Download a file from `url` and write it to `dest`.
///
/// Uses the kernel HTTP client with a 120-second timeout, streaming the
/// response body so large archives do not need to be buffered in memory.
///
/// # Errors
/// Returns an error if the HTTP request fails, the server returns a non-2xx
/// status, or writing the destination file fails.
pub fn download(url: &str, dest: &Path) -> Result<()> {
    let runtime = kernal_api::async_engine::RuntimeBuilder::current_thread()
        .enable_all()
        .build()
        .context("failed to build download runtime")?;
    let client = kernal_api::http::BlockingClient::new(
        &runtime,
        kernal_api::http::Limits {
            max_redirects: 10,
            max_body_bytes: 16 * 1024 * 1024 * 1024,
            ..kernal_api::http::Limits::default()
        },
    )
    .context("failed to build HTTP client")?;

    let mut response = client
        .get(url)
        .with_context(|| format!("GET {url} failed"))?;

    if !(200..300).contains(&response.status()) {
        crate::error_compat::bail!("server returned HTTP {} for {url}", response.status());
    }

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
    // FastLED toolchain artifacts must fit within this product-level bound.
    kernal_api::hash::sha256_file(path, 16 * 1024 * 1024 * 1024)
        .map(|digest| digest.to_hex())
        .with_context(|| format!("cannot hash {}", path.display()))
}

/// Return `true` when the SHA-256 digest of `path` matches `expected`.
///
/// `expected` is a 64-character hex string, compared case-insensitively.
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
/// The caller owns an absent or empty staging directory; failures may leave
/// partial output for the caller to clean up.
///
/// # Errors
/// Returns an error if the archive cannot be read or if any entry cannot be
/// written to `dest`.
pub fn extract_tar_zst(archive: &Path, dest: &Path) -> Result<()> {
    // The pinned Linux toolchain contains dangling npm links with literal
    // backslashes. Preserve their payloads, while requiring confined targets.
    kernal_api::archive::extract(
        archive,
        dest,
        ArchiveFormat::TarZstd,
        ExtractionLimits {
            dangling_links: DanglingLinks::PreserveMissingLeaf,
            ..ExtractionLimits::default()
        },
    )
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
/// Requires an absent or empty, caller-exclusive staging directory. Entries
/// are extracted preserving relative paths; failure may leave partial output.
///
/// # Errors
/// Returns an error if the archive cannot be read or any entry cannot be
/// written.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    kernal_api::archive::extract(
        archive,
        dest,
        ArchiveFormat::Zip,
        ExtractionLimits::default(),
    )
    .with_context(|| format!("failed to unpack zip archive to {}", dest.display()))
}

// ---------------------------------------------------------------------------
// Single-file extraction from a gzipped tar (.tgz)
// ---------------------------------------------------------------------------

/// Extract a single member from a `.tgz` archive into `dest`.
///
/// Used to pluck the esbuild binary out of an npm-style tarball (`package/...`
/// layout) without unpacking the whole archive.
/// The destination file must not exist; the caller owns cleanup on failure.
///
/// # Errors
/// Returns an error if the archive cannot be read or if `member` is not
/// present in the archive.
pub fn extract_member_from_tgz(archive: &Path, member: &str, dest: &Path) -> Result<()> {
    kernal_api::archive::extract_member(
        archive,
        member,
        dest,
        ArchiveFormat::TarGzip,
        ExtractionLimits::default(),
    )
    .with_context(|| format!("cannot extract {member} from {}", archive.display()))
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
/// `node_path` is the absolute path to the Node.js executable.  Emscripten
/// invokes this program from child processes, so a bare `node` name would
/// accidentally depend on the caller's shell PATH.
///
/// # Errors
/// Returns an error if the file cannot be written.
pub fn write_emscripten_config(install_dir: &Path, node_path: &Path) -> Result<()> {
    write_emscripten_config_at(install_dir, install_dir, node_path)
}

/// Write configuration for its final location while it is still staged.
pub(crate) fn write_emscripten_config_at(
    config_dir: &Path,
    install_dir: &Path,
    node_path: &Path,
) -> Result<()> {
    if !node_path.is_absolute() {
        crate::error_compat::bail!(
            "NODE_JS must be an absolute path, got {}",
            node_path.display()
        );
    }
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
         NODE_JS = {:?}\n\
         ",
        path_to_forward_slash(node_path),
    );

    let config_path = config_dir.join(".emscripten");
    if fs::read_to_string(&config_path).ok().as_deref() == Some(config.as_str()) {
        return Ok(());
    }
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
    use kernal_api::platform::fs::TemporaryDirectory;

    fn temp_dir() -> TemporaryDirectory {
        kernal_api::platform::fs::TemporaryDirectory::new().expect("tempdir")
    }

    #[test]
    fn download_writes_artifact_and_preserves_destination_on_http_error() {
        for status in [200, 404] {
            let dir = temp_dir();
            let destination = dir.path().join("artifact.zip");
            fs::write(&destination, b"previous artifact").unwrap();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/artifact.zip", listener.local_addr().unwrap());
            let worker = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                    assert!(request.len() < 4096);
                }
                write!(socket, "HTTP/1.1 {status} Fixture\r\nContent-Length: 8\r\nConnection: close\r\n\r\nartifact").unwrap();
            });
            let result = download(&url, &destination);
            worker.join().unwrap();
            if status == 200 {
                result.unwrap();
                assert_eq!(fs::read(destination).unwrap(), b"artifact");
            } else {
                assert!(result.is_err());
                assert_eq!(fs::read(destination).unwrap(), b"previous artifact");
            }
        }
    }

    // ------------------------------------------------------------------
    // SHA-256 verification
    // ------------------------------------------------------------------

    /// A known SHA-256 digest of the bytes `b"hello fastled"`.
    ///
    /// Fixed fixture; generic SHA-256 known-vector tests live in kernal-api.
    const HELLO_SHA256: &str = "9371e29d4390b284420993a4161cbda766dc36ce4d8396398a849d1de4d652d6";

    #[test]
    fn test_verify_sha256_correct() {
        let dir = temp_dir();
        let file = dir.path().join("data.bin");
        let content = b"hello fastled";
        fs::write(&file, content).unwrap();

        let ok = verify_sha256(&file, HELLO_SHA256).expect("verify_sha256");
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
    fn test_verify_sha256_accepts_uppercase_seal() {
        let dir = temp_dir();
        let file = dir.path().join("data.bin");
        fs::write(&file, b"hello fastled").unwrap();
        assert!(verify_sha256(&file, &HELLO_SHA256.to_uppercase()).unwrap());
    }

    #[test]
    fn test_verify_sha256_missing_artifact_is_an_error() {
        let dir = temp_dir();
        assert!(verify_sha256(&dir.path().join("missing.bin"), HELLO_SHA256).is_err());
    }

    // ------------------------------------------------------------------
    // .emscripten config generation
    // ------------------------------------------------------------------

    #[test]
    fn test_write_emscripten_config_paths_not_staging() {
        let dir = temp_dir();
        let install_dir = dir.path().join("emscripten").join("3.1.50");
        fs::create_dir_all(&install_dir).unwrap();

        let node = install_dir.join("managed-node");
        write_emscripten_config(&install_dir, &node).expect("write_emscripten_config");

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

        let node = dir.path().join("node");
        write_emscripten_config(&install_dir, &node).expect("write_emscripten_config");

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

        let node = dir.path().join("custom").join("path").join("node");
        write_emscripten_config(&install_dir, &node).expect("write_emscripten_config");

        let contents = fs::read_to_string(install_dir.join(".emscripten")).unwrap();
        assert!(
            contents.contains(&path_to_forward_slash(&node)),
            "config should contain the node path ({}): {contents}",
            node.display()
        );
    }

    #[test]
    fn test_write_emscripten_config_does_not_touch_unchanged_file() {
        let dir = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let install_dir = dir.path().join("emscripten").join("4.0.19");
        fs::create_dir_all(&install_dir).unwrap();
        let node = install_dir.join("managed-node");
        write_emscripten_config(&install_dir, &node).unwrap();
        let path = install_dir.join(".emscripten");
        let first = path.metadata().unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_emscripten_config(&install_dir, &node).unwrap();
        assert_eq!(first, path.metadata().unwrap().modified().unwrap());
    }

    #[test]
    fn test_write_emscripten_config_rejects_relative_node_path() {
        let dir = temp_dir();
        let install_dir = dir.path().join("emscripten").join("4.0.19");
        fs::create_dir_all(&install_dir).unwrap();

        let error = write_emscripten_config(&install_dir, Path::new("node"))
            .expect_err("bare node must be rejected")
            .to_string();
        assert!(error.contains("absolute path"), "got: {error}");
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
