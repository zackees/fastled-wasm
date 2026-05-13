//! Toolchain install entry points driven from the Rust CLI.
//!
//! Mirrors `src/fastled/toolchain/emscripten_archive.py` so the Python side
//! no longer needs `httpx` / `pyzstd` to materialise the emscripten toolchain.
//! Public entry points are intended to be called once at the top of the
//! compile flow; results are cached on disk via a `done.txt` marker.

use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ctcb_manifest::{PartRef, PlatformManifest};

use crate::archive;

const EMSCRIPTEN_MANIFEST_BASE_URL: &str =
    "https://raw.githubusercontent.com/zackees/clang-tool-chain-bins/main/assets/emscripten";

const ESBUILD_VERSION: &str = "0.28.0";

fn fastled_root() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot resolve home directory")?;
    Ok(home.join(".fastled"))
}

fn detect_platform_arch() -> (&'static str, &'static str) {
    let platform = if cfg!(target_os = "windows") {
        "win"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        std::env::consts::ARCH
    };
    (platform, arch)
}

/// Fetch and parse the platform manifest JSON.
fn fetch_manifest(url: &str) -> Result<PlatformManifest> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("failed to build HTTP client for manifest fetch")?;
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url} failed"))?
        .error_for_status()
        .with_context(|| format!("manifest fetch returned error for {url}"))?;
    let text = response.text().context("read manifest body")?;
    serde_json::from_str::<PlatformManifest>(&text)
        .with_context(|| format!("parse manifest JSON from {url}"))
}

/// Download each part to `parts_dir`, verify its SHA256, then concatenate the
/// parts into `merged_path`. Existing parts whose checksum still matches are
/// reused (no re-download).
fn download_multipart(parts: &[PartRef], parts_dir: &Path, merged_path: &Path) -> Result<()> {
    fs::create_dir_all(parts_dir)
        .with_context(|| format!("create parts dir {}", parts_dir.display()))?;
    let merged_file = fs::File::create(merged_path)
        .with_context(|| format!("create merged archive {}", merged_path.display()))?;
    let mut merged_writer = BufWriter::new(merged_file);

    for (index, part) in parts.iter().enumerate() {
        let part_path = parts_dir.join(format!("part-{:02}", index));
        if !part_path.exists() {
            archive::download(&part.href, &part_path)
                .with_context(|| format!("download part {}", part.href))?;
        }
        if !archive::verify_sha256(&part_path, &part.sha256)? {
            let actual = archive::sha256_file(&part_path).unwrap_or_default();
            let _ = fs::remove_file(&part_path);
            bail!(
                "checksum mismatch for {}: got {}, expected {}",
                part.href,
                actual,
                part.sha256
            );
        }
        let mut part_reader = BufReader::new(
            fs::File::open(&part_path)
                .with_context(|| format!("open part {}", part_path.display()))?,
        );
        std::io::copy(&mut part_reader, &mut merged_writer).with_context(|| {
            format!(
                "concatenate part {} into {}",
                part_path.display(),
                merged_path.display()
            )
        })?;
    }
    merged_writer.flush()?;
    Ok(())
}

/// Ensure the emscripten toolchain is installed at
/// `~/.fastled/toolchains/emscripten/{platform}/{arch}/{version}/`.
/// Returns the install directory path.
///
/// The layout intentionally matches the Python implementation so a previously
/// Python-installed toolchain is picked up without re-downloading.
pub fn ensure_emscripten_installed() -> Result<PathBuf> {
    let (platform, arch) = detect_platform_arch();
    let root = fastled_root()?;
    let install_base = root
        .join("toolchains")
        .join("emscripten")
        .join(platform)
        .join(arch);
    let cache_dir = root.join("toolchains").join("archives");
    fs::create_dir_all(&install_base)?;
    fs::create_dir_all(&cache_dir)?;

    let manifest_url = format!("{EMSCRIPTEN_MANIFEST_BASE_URL}/{platform}/{arch}/manifest.json");
    let manifest = fetch_manifest(&manifest_url)?;
    let version = manifest.latest.clone();
    let install_dir = install_base.join(&version);
    let done_file = install_dir.join("done.txt");
    if done_file.exists() {
        return Ok(install_dir);
    }

    let version_info = manifest
        .versions
        .get(&version)
        .with_context(|| format!("manifest has no entry for version {version}"))?;

    let archive_path = cache_dir.join(format!("emscripten-{platform}-{arch}-{version}.tar.zst"));
    let parts_subdir = cache_dir.join(format!("emscripten-{platform}-{arch}-{version}.parts"));

    if !archive_path.exists() {
        let parts = version_info
            .parts
            .as_ref()
            .with_context(|| format!("manifest version {version} has no parts"))?;
        download_multipart(parts, &parts_subdir, &archive_path)?;
    }

    if !archive::verify_sha256(&archive_path, &version_info.sha256)? {
        let actual = archive::sha256_file(&archive_path).unwrap_or_default();
        let _ = fs::remove_file(&archive_path);
        bail!(
            "archive checksum mismatch for {}: got {}, expected {}",
            version_info.href,
            actual,
            version_info.sha256
        );
    }

    // Atomic-ish install via staging directory.
    let staging = install_base.join(format!("{version}.staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    archive::extract_tar_zst(&archive_path, &staging)?;

    fs::write(staging.join("done.txt"), "ok\n")?;

    if install_dir.exists() {
        fs::remove_dir_all(&install_dir)?;
    }
    fs::rename(&staging, &install_dir)?;

    archive::write_emscripten_config(&install_dir, "node")?;

    Ok(install_dir)
}

// ---------------------------------------------------------------------------
// esbuild
// ---------------------------------------------------------------------------

/// npm package platform-arch strings differ from the emscripten ones.
fn esbuild_platform_arch() -> Result<(&'static str, &'static str)> {
    let platform = if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        anyhow::bail!(
            "unsupported architecture for esbuild: {}",
            std::env::consts::ARCH
        );
    };
    Ok((platform, arch))
}

/// Ensure the esbuild binary is installed at
/// `~/.fastled/toolchains/esbuild/{platform}/{arch}/{version}/`.
/// Returns the path to the executable.
///
/// Layout matches the previous Python implementation
/// (`src/fastled/frontend_esbuild.py`).
pub fn ensure_esbuild_installed() -> Result<PathBuf> {
    let (platform, arch) = esbuild_platform_arch()?;
    let version = ESBUILD_VERSION;
    let root = fastled_root()?;
    let install_dir = root
        .join("toolchains")
        .join("esbuild")
        .join(platform)
        .join(arch)
        .join(version);
    let exe_name = if cfg!(target_os = "windows") {
        "esbuild.exe"
    } else {
        "esbuild"
    };
    let esbuild_path = install_dir.join(exe_name);
    let done_file = install_dir.join("done.txt");
    if done_file.exists() && esbuild_path.exists() {
        return Ok(esbuild_path);
    }
    fs::create_dir_all(&install_dir)?;

    let archive_cache = root.join("toolchains").join("archives");
    fs::create_dir_all(&archive_cache)?;
    let archive_path = archive_cache.join(format!("esbuild-{platform}-{arch}-{version}.tgz"));
    if !archive_path.exists() {
        let url = format!(
            "https://registry.npmjs.org/@esbuild/{platform}-{arch}/-/{platform}-{arch}-{version}.tgz"
        );
        archive::download(&url, &archive_path)?;
    }

    let member = if cfg!(target_os = "windows") {
        format!("package/{exe_name}")
    } else {
        "package/bin/esbuild".to_string()
    };

    if esbuild_path.exists() {
        let _ = fs::remove_file(&esbuild_path);
    }
    archive::extract_member_from_tgz(&archive_path, &member, &esbuild_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&esbuild_path)?.permissions();
        perms.set_mode(perms.mode() | 0o111);
        fs::set_permissions(&esbuild_path, perms)?;
    }

    fs::write(&done_file, "ok\n")?;
    Ok(esbuild_path)
}
