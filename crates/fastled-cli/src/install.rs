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
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::archive;

// ---------------------------------------------------------------------------
// Manifest schema
// ---------------------------------------------------------------------------
//
// The clang-tool-chain-bins emscripten manifest is served in two different
// shapes depending on platform. Linux/macOS nests version entries under a
// `versions` key:
//
//     { "latest": "4.0.21",
//       "versions": { "4.0.21": { "href": "...", "sha256": "...", "parts": [...] }, ... } }
//
// Windows inlines version entries at the top level alongside `latest`:
//
//     { "latest": "4.0.19",
//       "4.0.19": { "href": "...", "sha256": "...", "parts": [...] } }
//
// Our `PlatformManifest` accepts both via a custom Deserialize.

#[derive(Debug, Deserialize)]
struct PartRef {
    href: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct VersionInfo {
    href: String,
    sha256: String,
    parts: Option<Vec<PartRef>>,
}

#[derive(Debug)]
struct PlatformManifest {
    latest: String,
    versions: BTreeMap<String, VersionInfo>,
}

impl<'de> Deserialize<'de> for PlatformManifest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw: serde_json::Value = Deserialize::deserialize(deserializer)?;
        let obj = raw
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("expected JSON object at manifest root"))?;

        let latest = obj
            .get("latest")
            .and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::custom("missing 'latest' key"))?
            .to_string();

        let mut versions = BTreeMap::new();

        // Prefer the nested-`versions` shape (Linux / macOS manifests) when
        // present. Fall back to top-level inline entries (Windows).
        if let Some(serde_json::Value::Object(versions_obj)) = obj.get("versions") {
            for (key, value) in versions_obj {
                let info: VersionInfo = serde_json::from_value(value.clone()).map_err(|e| {
                    serde::de::Error::custom(format!(
                        "bad version entry '{key}' (nested versions): {e}"
                    ))
                })?;
                versions.insert(key.clone(), info);
            }
        } else {
            for (key, value) in obj {
                if key == "latest" {
                    continue;
                }
                let info: VersionInfo = serde_json::from_value(value.clone()).map_err(|e| {
                    serde::de::Error::custom(format!(
                        "bad version entry '{key}' (inline versions): {e}"
                    ))
                })?;
                versions.insert(key.clone(), info);
            }
        }

        Ok(PlatformManifest { latest, versions })
    }
}

const EMSCRIPTEN_MANIFEST_BASE_URL: &str =
    "https://raw.githubusercontent.com/zackees/clang-tool-chain-bins/main/assets/emscripten";

const ESBUILD_VERSION: &str = "0.28.0";

const FASTLED_REPO: &str = "FastLED/FastLED";
const FASTLED_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/FastLED/FastLED/releases/latest";

const CHROME_CRX_URL: &str = "https://clients2.google.com/service/update2/crx?response=redirect&prodversion=114.0&acceptformat=crx2,crx3&x=id%3D{ID}%26uc";
const CHROME_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36";

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

    // Restore execute bits on Unix — some tar implementations (and the
    // umask of the extracting process) strip the +x bit from binaries
    // under bin/, emscripten/, and libexec/. emcc.py then hits
    // PermissionError when it tries to exec clang. Mirror the
    // belt-and-suspenders chmod from the previous Python implementation.
    #[cfg(unix)]
    {
        for subdir in ["bin", "emscripten", "libexec"] {
            let dir = staging.join(subdir);
            if dir.is_dir() {
                add_executable_bits_recursive(&dir)?;
            }
        }
    }

    fs::write(staging.join("done.txt"), "ok\n")?;

    if install_dir.exists() {
        fs::remove_dir_all(&install_dir)?;
    }
    fs::rename(&staging, &install_dir)?;

    archive::write_emscripten_config(&install_dir, "node")?;

    Ok(install_dir)
}

/// Recursively chmod every file under `dir` to include +x for user / group /
/// other (no-op on Windows). Used right after emscripten extraction so the
/// shipped binaries (clang, wasm-ld, etc.) are actually executable.
#[cfg(unix)]
fn add_executable_bits_recursive(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            add_executable_bits_recursive(&path)?;
        } else if file_type.is_file() {
            let mode = entry.metadata()?.permissions().mode();
            let new_mode = mode | 0o111;
            if new_mode != mode {
                fs::set_permissions(&path, fs::Permissions::from_mode(new_mode))?;
            }
        }
    }
    Ok(())
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

// ---------------------------------------------------------------------------
// FastLED repo download (used by --init)
// ---------------------------------------------------------------------------

fn is_commit_sha(ref_str: &str) -> bool {
    let n = ref_str.len();
    (7..=40).contains(&n) && ref_str.chars().all(|c| c.is_ascii_hexdigit())
}

/// Hit the GitHub API for the latest FastLED release tag.
/// Returns `None` on any failure so callers can fall back to `master`.
fn fetch_latest_release_tag() -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .ok()?;
    let resp = client
        .get(FASTLED_LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "fastled-cli")
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn head_check(url: &str) -> bool {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build();
    match client {
        Ok(c) => c
            .head(url)
            .header("User-Agent", "fastled-cli")
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Resolve `ref` to `(display_name, archive_url)`. Mirrors
/// `project_init._resolve_ref` in Python.
fn resolve_fastled_ref(ref_opt: Option<&str>) -> (String, String) {
    let archive_base = format!("https://github.com/{FASTLED_REPO}/archive");

    match ref_opt {
        None | Some("latest_release") => match fetch_latest_release_tag() {
            Some(tag) => {
                let url = format!("{archive_base}/refs/tags/{tag}.zip");
                (tag, url)
            }
            None => {
                eprintln!(
                    "fastled: could not fetch latest FastLED release tag, falling back to master"
                );
                let url = format!("{archive_base}/refs/heads/master.zip");
                ("master".to_string(), url)
            }
        },
        Some(r) if is_commit_sha(r) => {
            let url = format!("{archive_base}/{r}.zip");
            (r.to_string(), url)
        }
        Some(r) => {
            // Try as tag first, fall back to branch (mirrors Python).
            let tag_url = format!("{archive_base}/refs/tags/{r}.zip");
            if head_check(&tag_url) {
                (r.to_string(), tag_url)
            } else {
                let branch_url = format!("{archive_base}/refs/heads/{r}.zip");
                (r.to_string(), branch_url)
            }
        }
    }
}

/// Locate the root of an extracted FastLED archive (e.g. `FastLED-master`,
/// `FastLED-3.9.12`, `FastLED-<sha>`).
fn find_fastled_extract_root(dir: &Path) -> Result<PathBuf> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.file_name().to_string_lossy().starts_with("FastLED")
        {
            return Ok(entry.path());
        }
    }
    anyhow::bail!("no FastLED* directory found inside {}", dir.display())
}

/// Ensure the FastLED repo for `ref_opt` is downloaded and extracted under
/// `~/.fastled/cache/fastled-{ref}/`. Returns the resolved local repo root.
///
/// Re-uses an existing extraction if `library.json` is already present, so
/// repeated calls are cheap.
pub fn ensure_fastled_repo(ref_opt: Option<&str>) -> Result<PathBuf> {
    let (ref_name, url) = resolve_fastled_ref(ref_opt);
    let root = fastled_root()?;
    let cache_base = root.join("cache");
    fs::create_dir_all(&cache_base)?;
    let repo_dir = cache_base.join(format!("fastled-{ref_name}"));

    if repo_dir.join("library.json").is_file() {
        return Ok(repo_dir);
    }

    let archive_cache = cache_base.join("archives");
    fs::create_dir_all(&archive_cache)?;
    let archive_path = archive_cache.join(format!("FastLED-{ref_name}.zip"));
    if !archive_path.exists() {
        archive::download(&url, &archive_path)
            .with_context(|| format!("download FastLED archive from {url}"))?;
    }

    // Extract to a staging dir so a partial extraction never poisons the final
    // location.
    let staging = cache_base.join(format!("fastled-{ref_name}.staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    archive::extract_zip(&archive_path, &staging)?;

    let extracted_root = find_fastled_extract_root(&staging)?;

    if repo_dir.exists() {
        fs::remove_dir_all(&repo_dir)?;
    }
    fs::rename(&extracted_root, &repo_dir)?;
    fs::remove_dir_all(&staging).ok();

    Ok(repo_dir)
}

// ---------------------------------------------------------------------------
// Chrome extension (CRX) download
// ---------------------------------------------------------------------------

/// Download a CRX from the Chrome Web Store, strip the CRX header, extract
/// the embedded ZIP into ``~/.fastled/chrome-extensions/{name}/``, and return
/// the install path.
///
/// Reuses the cached extension if `manifest.json` already exists at the
/// destination.
pub fn ensure_chrome_extension(extension_id: &str, name: &str) -> Result<PathBuf> {
    let root = fastled_root()?;
    let install_dir = root.join("chrome-extensions").join(name);
    if install_dir.join("manifest.json").is_file() {
        return Ok(install_dir);
    }
    fs::create_dir_all(&install_dir)
        .with_context(|| format!("create chrome extension dir {}", install_dir.display()))?;

    let crx_url = CHROME_CRX_URL.replace("{ID}", extension_id);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("failed to build HTTP client for CRX download")?;

    let bytes = client
        .get(&crx_url)
        .header("User-Agent", CHROME_USER_AGENT)
        .header("Referer", "https://chrome.google.com")
        .send()
        .with_context(|| format!("GET {crx_url} failed"))?
        .error_for_status()
        .with_context(|| format!("CRX download returned error for {crx_url}"))?
        .bytes()
        .context("read CRX body")?;

    // CRX file starts with a header; find the embedded ZIP magic.
    let buf: &[u8] = &bytes;
    let zip_start = find_zip_start(buf)
        .with_context(|| format!("no ZIP magic found in CRX for {extension_id}"))?;

    // Write the ZIP portion to a temp file and extract.
    let tmp_zip = tempfile::NamedTempFile::new().context("create temp zip for CRX extraction")?;
    {
        use std::io::Write;
        let mut handle = tmp_zip.as_file();
        handle
            .write_all(&buf[zip_start..])
            .context("write CRX zip portion")?;
        handle.flush().ok();
    }
    archive::extract_zip(tmp_zip.path(), &install_dir)?;

    if !install_dir.join("manifest.json").is_file() {
        let _ = fs::remove_dir_all(&install_dir);
        anyhow::bail!(
            "extension extraction failed: manifest.json not found in {}",
            install_dir.display()
        );
    }
    Ok(install_dir)
}

fn find_zip_start(bytes: &[u8]) -> Option<usize> {
    // PK\x03\x04 is a normal ZIP local file header; PK\x05\x06 is an empty
    // central directory end-of-archive marker. Mirror the Python search.
    let local = [0x50u8, 0x4B, 0x03, 0x04];
    let end = [0x50u8, 0x4B, 0x05, 0x06];
    bytes
        .windows(4)
        .position(|w| w == local)
        .or_else(|| bytes.windows(4).position(|w| w == end))
}
