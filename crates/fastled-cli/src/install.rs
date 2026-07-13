//! Toolchain install entry points driven from the Rust CLI.
//!
//! Mirrors `src/fastled/toolchain/emscripten_archive.py` so the Python side
//! no longer needs `httpx` / `pyzstd` to materialise the emscripten toolchain.
//! Public entry points are intended to be called once at the top of the
//! compile flow; results are cached on disk via a `done.txt` marker.

use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use ctcb_core::Target;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::archive;

/// Toolchain platform manifest as published in
/// `clang-tool-chain-bins/assets/emscripten/{platform}/{arch}/manifest.json`.
///
/// Local mirror of the schema rather than relying on `ctcb-manifest` because
/// the published manifests use a `"versions": { … }` map, while older
/// `ctcb-manifest` releases expected version keys at the top level. Keeping
/// the schema local insulates the CLI from upstream crate drift.
#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlatformManifest {
    latest: String,
    versions: BTreeMap<String, VersionInfo>,
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VersionInfo {
    href: String,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parts: Option<Vec<PartRef>>,
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PartRef {
    href: String,
    sha256: String,
}

const ESBUILD_VERSION: &str = "0.28.0";
const EMSCRIPTEN_VERSION_MARKER: &str = ".fastled-manifest-version";

static EMSCRIPTEN_INSTALL_CACHE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

const FASTLED_REPO: &str = "FastLED/FastLED";
const FASTLED_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/FastLED/FastLED/releases/latest";

fn fastled_root() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot resolve home directory")?;
    Ok(home.join(".fastled"))
}

fn detect_platform_arch() -> Result<(String, String)> {
    let target = Target::current().context("detect host clang-tool-chain target")?;
    Ok((target.platform.to_string(), target.arch.to_string()))
}

/// Parse a platform manifest, accepting both the current schema (`versions`
/// sub-map) and the historical layout where version keys live at the top
/// level next to `latest`. The two formats coexist on the assets server
/// today, so the CLI has to handle both.
#[cfg(test)]
fn parse_platform_manifest(text: &str) -> Result<PlatformManifest> {
    if let Ok(parsed) = serde_json::from_str::<PlatformManifest>(text) {
        return Ok(parsed);
    }

    let value: serde_json::Value = serde_json::from_str(text)?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("manifest is not a JSON object"))?;

    let latest = object
        .get("latest")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("manifest is missing required field `latest`"))?
        .to_string();

    let mut versions = BTreeMap::new();
    for (key, value) in object {
        if key == "latest" {
            continue;
        }
        let info: VersionInfo = serde_json::from_value(value.clone())
            .with_context(|| format!("parse version entry `{key}`"))?;
        versions.insert(key.clone(), info);
    }

    if versions.is_empty() {
        bail!("manifest does not declare any versions");
    }
    if !versions.contains_key(&latest) {
        bail!("manifest `latest`={latest} has no matching version entry");
    }

    Ok(PlatformManifest { latest, versions })
}

#[cfg(test)]
fn multipart_parts(version_info: &VersionInfo) -> Option<&[PartRef]> {
    version_info
        .parts
        .as_deref()
        .filter(|parts| !parts.is_empty())
}

#[cfg(unix)]
fn ensure_toolchain_executables(install_dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fn add_execute_bits(path: &Path) -> Result<()> {
        let metadata =
            fs::metadata(path).with_context(|| format!("read metadata for {}", path.display()))?;
        if !metadata.is_file() {
            return Ok(());
        }
        let mut permissions = metadata.permissions();
        let mode = permissions.mode();
        if mode & 0o111 == 0 {
            permissions.set_mode(mode | 0o111);
            fs::set_permissions(path, permissions)
                .with_context(|| format!("set executable bit on {}", path.display()))?;
        }
        Ok(())
    }

    let bin_dir = install_dir.join("bin");
    let mut pending = if bin_dir.is_dir() {
        vec![bin_dir]
    } else {
        Vec::new()
    };
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
            let path = entry.path();
            let metadata = fs::metadata(&path)
                .with_context(|| format!("read metadata for {}", path.display()))?;
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }

            add_execute_bits(&path)?;
        }
    }

    // Emscripten invokes these extensionless launchers directly while
    // building system libraries. They live outside bin/, and some published
    // archives lose their Unix mode bits.
    for name in [
        "em++",
        "emar",
        "embuilder",
        "emcc",
        "emcmake",
        "emconfigure",
        "emmake",
        "emnm",
        "emranlib",
        "emrun",
        "emsize",
        "emstrip",
    ] {
        let launcher = install_dir.join("emscripten").join(name);
        if launcher.exists() {
            add_execute_bits(&launcher)?;
        }
    }

    Ok(())
}

#[cfg(not(unix))]
fn ensure_toolchain_executables(_install_dir: &Path) -> Result<()> {
    Ok(())
}

fn required_emscripten_payload_files(install_dir: &Path) -> Vec<PathBuf> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    vec![
        install_dir.join("emscripten/emcc.py"),
        install_dir.join("emscripten/em++.py"),
        install_dir.join("emscripten/emar.py"),
        install_dir.join("emscripten/emscripten-version.txt"),
        install_dir.join(format!("bin/clang++{suffix}")),
        install_dir.join(format!("bin/wasm-ld{suffix}")),
        install_dir.join(format!("bin/llvm-ar{suffix}")),
        install_dir.join(format!("bin/llvm-objcopy{suffix}")),
        install_dir.join(format!("bin/wasm-emscripten-finalize{suffix}")),
    ]
}

fn validate_emscripten_payload(install_dir: &Path) -> Result<()> {
    let missing = required_emscripten_payload_files(install_dir)
        .into_iter()
        .filter(|path| {
            !fs::metadata(path)
                .map(|metadata| metadata.is_file() && metadata.len() > 0)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    bail!(
        "Emscripten installation is incomplete; missing or empty files: {}",
        missing
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn validate_complete_emscripten_install(install_dir: &Path) -> Result<()> {
    validate_emscripten_payload(install_dir)?;
    let done = install_dir.join("done.txt");
    if !done.is_file()
        || fs::metadata(&done)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            == 0
    {
        bail!(
            "Emscripten installation is missing a complete marker: {}",
            done.display()
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolchainSpec {
    pub platform: &'static str,
    pub arch: &'static str,
    pub package_id: &'static str,
    pub archive_url: &'static str,
    pub archive_sha256: &'static str,
    pub archive_parts: &'static [ToolchainPart],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolchainPart {
    pub url: &'static str,
    pub sha256: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ToolchainReceipt {
    schema_version: u32,
    #[serde(default)]
    catalog_commit: String,
    platform: String,
    arch: String,
    package_id: String,
    archive_url: String,
    archive_sha256: String,
    health_checked: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct ActiveToolchainState {
    schema_version: u32,
    active: Option<String>,
    previous_known_good: Option<String>,
}

const TOOLCHAIN_STATE_SCHEMA: u32 = 1;
const TOOLCHAIN_RECEIPT_SCHEMA: u32 = 1;
const TOOLCHAIN_CATALOG_COMMIT: &str = "ef4a0e4a767c46528776105815033fb870ec337a";
const TOOLCHAIN_STATE_FILE: &str = ".fastled-active-toolchain.json";
const TOOLCHAIN_RECEIPT_FILE: &str = ".fastled-toolchain.json";

pub fn release_default_toolchain() -> Result<ToolchainSpec> {
    let (platform, arch) = detect_platform_arch()?;
    catalog_toolchain(&platform, &arch)
}

fn catalog_toolchain(platform: &str, arch: &str) -> Result<ToolchainSpec> {
    let commit = TOOLCHAIN_CATALOG_COMMIT;
    match (platform, arch) {
        ("win", "x86_64") => Ok(ToolchainSpec {
            platform: "win",
            arch: "x86_64",
            package_id: "4.0.19",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/win/x86_64/emscripten-latest-win-x86_64.tar.zst",
            archive_sha256: "b19c2e35b863eb17866034f917d7957514645e179e9d22800729b0dcbb2aa2e2",
            archive_parts: &[],
        }),
        ("linux", "x86_64") => Ok(ToolchainSpec {
            platform: "linux",
            arch: "x86_64",
            package_id: "4.0.21",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/linux/x86_64/emscripten-4.0.21-linux-x86_64.tar.zst",
            archive_sha256: "5cd3cbe0316d37c9b39bdc63691c014f136a5d82a9f08ed29bb7ad62f7a83655",
            archive_parts: &[],
        }),
        ("linux", "arm64") => Ok(ToolchainSpec {
            platform: "linux",
            arch: "arm64",
            package_id: "4.0.21",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/linux/arm64/emscripten-4.0.21-linux-arm64.tar.zst",
            archive_sha256: "610375cc8e88fabe47a1675e747e8aade31279eb1e6ec2bad6a355e6376af16f",
            archive_parts: &[],
        }),
        ("darwin", "x86_64") => Ok(ToolchainSpec {
            platform: "darwin",
            arch: "x86_64",
            package_id: "releases-d70a5da89b3e673bf6a482724478fc17e81e575e",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/darwin/x86_64/emscripten-releases-d70a5da89b3e673bf6a482724478fc17e81e575e-darwin-x86_64.tar.zst",
            archive_sha256: "6ba74e00642568383798a7ccd3b643ce3c5cd5606789bbe824aa9971f0d8894f",
            archive_parts: &[],
        }),
        ("darwin", "arm64") => Ok(ToolchainSpec {
            platform: "darwin",
            arch: "arm64",
            package_id: "releases-d70a5da89b3e673bf6a482724478fc17e81e575e",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/darwin/arm64/emscripten-releases-d70a5da89b3e673bf6a482724478fc17e81e575e-darwin-arm64.tar.zst",
            archive_sha256: "6af749f0d44927c7d4c93c9e407e195257ed690b8e3bd3a43d7a3f4badc52082",
            archive_parts: &[],
        }),
        _ => bail!(
            "unsupported Emscripten target {platform}/{arch} in catalog commit {commit}"
        ),
    }
}

fn toolchain_root() -> Result<PathBuf> {
    Ok(fastled_root()?.join("toolchains").join("emscripten"))
}

fn install_base(spec: ToolchainSpec) -> Result<PathBuf> {
    Ok(toolchain_root()?.join(spec.platform).join(spec.arch))
}

fn package_key(spec: ToolchainSpec) -> String {
    format!("{}-{}", spec.package_id, &spec.archive_sha256[..12])
}

fn package_dir(base: &Path, spec: ToolchainSpec) -> PathBuf {
    base.join(package_key(spec))
}

fn state_path(base: &Path) -> PathBuf {
    base.join(TOOLCHAIN_STATE_FILE)
}

fn receipt_path(install: &Path) -> PathBuf {
    install.join(TOOLCHAIN_RECEIPT_FILE)
}

fn read_state(base: &Path) -> Result<ActiveToolchainState> {
    let path = state_path(base);
    if !path.is_file() {
        return Ok(ActiveToolchainState {
            schema_version: TOOLCHAIN_STATE_SCHEMA,
            ..ActiveToolchainState::default()
        });
    }
    let state: ActiveToolchainState = serde_json::from_str(
        &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))?;
    if state.schema_version != TOOLCHAIN_STATE_SCHEMA {
        bail!(
            "unsupported toolchain state schema {}",
            state.schema_version
        );
    }
    Ok(state)
}

fn write_state(base: &Path, state: &ActiveToolchainState) -> Result<()> {
    let path = state_path(base);
    let temp = base.join(format!(".toolchain-state-{}.tmp", std::process::id()));
    fs::write(&temp, serde_json::to_vec_pretty(state)?)?;
    if path.exists() {
        let backup = base.join(format!(".toolchain-state-{}.bak", std::process::id()));
        fs::rename(&path, &backup).with_context(|| format!("backup {}", path.display()))?;
        if let Err(error) = fs::rename(&temp, &path) {
            let _ = fs::rename(&backup, &path);
            return Err(error).with_context(|| format!("publish {}", path.display()));
        }
        let _ = fs::remove_file(backup);
    } else {
        fs::rename(&temp, &path).with_context(|| format!("publish {}", path.display()))?;
    }
    Ok(())
}

fn write_receipt(install: &Path, spec: ToolchainSpec, health_checked: bool) -> Result<()> {
    let receipt = ToolchainReceipt {
        schema_version: TOOLCHAIN_RECEIPT_SCHEMA,
        catalog_commit: TOOLCHAIN_CATALOG_COMMIT.to_string(),
        platform: spec.platform.to_string(),
        arch: spec.arch.to_string(),
        package_id: spec.package_id.to_string(),
        archive_url: spec.archive_url.to_string(),
        archive_sha256: spec.archive_sha256.to_string(),
        health_checked,
    };
    fs::write(receipt_path(install), serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

fn read_receipt(install: &Path) -> Result<ToolchainReceipt> {
    let path = receipt_path(install);
    serde_json::from_str(
        &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))
}

fn validate_managed_install(install: &Path, spec: ToolchainSpec) -> Result<()> {
    validate_complete_emscripten_install(install)?;
    let receipt = read_receipt(install)?;
    if receipt.platform != spec.platform
        || receipt.arch != spec.arch
        || receipt.package_id != spec.package_id
        || receipt.archive_sha256 != spec.archive_sha256
    {
        bail!(
            "toolchain receipt does not match catalog package {}",
            spec.package_id
        );
    }
    Ok(())
}

fn has_install_history(base: &Path) -> bool {
    fs::read_dir(base)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            entry.path().is_dir() || name == EMSCRIPTEN_VERSION_MARKER
        })
}

fn migrate_legacy_install(base: &Path, spec: ToolchainSpec) -> Result<Option<PathBuf>> {
    let mut candidates = Vec::new();
    let marker = base.join(EMSCRIPTEN_VERSION_MARKER);
    if let Ok(version) = fs::read_to_string(&marker) {
        candidates.push(base.join(version.trim()));
    }
    candidates.push(base.join(spec.package_id));
    candidates.push(base.join(spec.package_id.split('-').next().unwrap_or(spec.package_id)));

    for candidate in candidates {
        if candidate.is_dir()
            && validate_complete_emscripten_install(&candidate).is_ok()
            && candidate.file_name().is_some_and(|name| {
                name == spec.package_id || name == spec.package_id.split('-').next().unwrap_or("")
            })
        {
            write_receipt(&candidate, spec, false)?;
            let state = ActiveToolchainState {
                schema_version: TOOLCHAIN_STATE_SCHEMA,
                active: Some(
                    candidate
                        .strip_prefix(base)
                        .unwrap_or(&candidate)
                        .to_string_lossy()
                        .into_owned(),
                ),
                previous_known_good: None,
            };
            write_state(base, &state)?;
            fs::remove_file(marker).ok();
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn resolve_active_install(base: &Path, spec: ToolchainSpec) -> Result<Option<PathBuf>> {
    fs::create_dir_all(base)?;
    let mut state = read_state(base)?;
    if state.active.is_none() {
        if let Some(legacy) = migrate_legacy_install(base, spec)? {
            return Ok(Some(legacy));
        }
        return Ok(None);
    }

    let active = base.join(state.active.as_deref().unwrap_or_default());
    if validate_managed_install(&active, spec).is_ok() {
        return Ok(Some(active));
    }
    if let Some(previous_key) = state.previous_known_good.take() {
        let previous = base.join(&previous_key);
        if validate_managed_install(&previous, spec).is_ok() {
            return Ok(Some(previous));
        }
    }
    bail!("active Emscripten toolchain is missing or invalid; run `fastled toolchain repair`")
}

fn atomic_download(url: &str, destination: &Path) -> Result<()> {
    let partial = destination.with_extension("partial");
    if partial.exists() {
        fs::remove_file(&partial).ok();
    }
    archive::download(url, &partial).with_context(|| format!("download archive {url}"))?;
    fs::rename(&partial, destination)
        .with_context(|| format!("publish downloaded archive {}", destination.display()))?;
    Ok(())
}

fn download_multipart(
    parts: &[ToolchainPart],
    cache_dir: &Path,
    archive_path: &Path,
) -> Result<()> {
    let merged_partial = archive_path.with_extension("partial");
    let merged_file = fs::File::create(&merged_partial)
        .with_context(|| format!("create partial archive {}", merged_partial.display()))?;
    let mut merged_writer = BufWriter::new(merged_file);
    for (index, part) in parts.iter().enumerate() {
        let part_path = cache_dir.join(format!(
            "{}.part-{index:02}",
            package_key_for_archive(archive_path)
        ));
        if !part_path.is_file() || !archive::verify_sha256(&part_path, part.sha256).unwrap_or(false)
        {
            if part_path.exists() {
                fs::remove_file(&part_path).ok();
            }
            atomic_download(part.url, &part_path)?;
        }
        if !archive::verify_sha256(&part_path, part.sha256)? {
            fs::remove_file(&part_path).ok();
            bail!("checksum mismatch for multipart package part {}", index);
        }
        let mut reader = BufReader::new(fs::File::open(&part_path)?);
        std::io::copy(&mut reader, &mut merged_writer)?;
    }
    merged_writer.flush()?;
    fs::rename(&merged_partial, archive_path)
        .with_context(|| format!("publish multipart archive {}", archive_path.display()))?;
    Ok(())
}

fn package_key_for_archive(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("emscripten")
        .to_string()
}

fn resolve_node() -> Option<String> {
    if cfg!(windows) {
        Some("node.exe".to_string())
    } else {
        Some("node".to_string())
    }
}

fn run_health_checks(install: &Path) -> Result<()> {
    let temp =
        std::env::temp_dir().join(format!("fastled-toolchain-health-{}", std::process::id()));
    if temp.exists() {
        fs::remove_dir_all(&temp).ok();
    }
    fs::create_dir_all(&temp)?;
    let result = (|| -> Result<()> {
        let empp = install.join("emscripten").join("em++.py");
        let python = if cfg!(windows) {
            "python.exe"
        } else {
            "python3"
        };
        let run = |args: &[&str]| -> Result<()> {
            let status = Command::new(python)
                .arg(&empp)
                .args(args)
                .env("EM_CONFIG", install.join(".emscripten"))
                .env("EMSCRIPTEN", install.join("emscripten"))
                .current_dir(&temp)
                .status()
                .context("run Emscripten health check")?;
            if !status.success() {
                bail!(
                    "Emscripten health-check command failed: em++ {}",
                    args.join(" ")
                );
            }
            Ok(())
        };
        fs::write(temp.join("static.cpp"), "int main() { return 0; }\n")?;
        run(&[
            "static.cpp",
            "-O0",
            "-sWASM_BIGINT=1",
            "-sEXIT_RUNTIME=1",
            "-o",
            "static.js",
        ])?;
        let node = resolve_node().context("Node.js is required for Emscripten health checks")?;
        let static_status = Command::new(&node)
            .arg(temp.join("static.js"))
            .current_dir(&temp)
            .status()
            .context("run static Emscripten health check")?;
        if !static_status.success() {
            bail!("static Emscripten health check failed");
        }

        fs::write(temp.join("side.cpp"), "int side_value() { return 42; }\n")?;
        fs::write(
            temp.join("main.cpp"),
            "extern int side_value(); int main() { return side_value() == 42 ? 0 : 1; }\n",
        )?;
        run(&[
            "side.cpp",
            "-O0",
            "-sSIDE_MODULE=1",
            "-sWASM_BIGINT=1",
            "-o",
            "side.wasm",
        ])?;
        run(&[
            "main.cpp",
            "side.wasm",
            "-O0",
            "-sMAIN_MODULE=2",
            "-sWASM_BIGINT=1",
            "-sEXIT_RUNTIME=1",
            "-o",
            "dynamic.js",
        ])?;
        let dynamic_status = Command::new(&node)
            .arg(temp.join("dynamic.js"))
            .current_dir(&temp)
            .status()
            .context("run dynamic Emscripten health check")?;
        if !dynamic_status.success() {
            bail!("dynamic Emscripten health check failed");
        }
        Ok(())
    })();
    fs::remove_dir_all(&temp).ok();
    result
}

fn install_spec(
    spec: ToolchainSpec,
    health_check: bool,
    replace_invalid: bool,
    force_redownload: bool,
) -> Result<PathBuf> {
    let base = install_base(spec)?;
    let root = fastled_root()?;
    let cache_dir = root.join("toolchains").join("archives");
    fs::create_dir_all(&base)?;
    fs::create_dir_all(&cache_dir)?;
    let destination = package_dir(&base, spec);
    if !force_redownload && validate_managed_install(&destination, spec).is_ok() {
        if health_check {
            run_health_checks(&destination)?;
        }
        return Ok(destination);
    }
    let archive_path = cache_dir.join(format!("emscripten-{}.tar.zst", package_key(spec)));
    if force_redownload && archive_path.exists() {
        fs::remove_file(&archive_path).ok();
    }
    if !archive_path.is_file()
        || !archive::verify_sha256(&archive_path, spec.archive_sha256).unwrap_or(false)
    {
        if archive_path.exists() {
            fs::remove_file(&archive_path).ok();
        }
        if spec.archive_parts.is_empty() {
            atomic_download(spec.archive_url, &archive_path)?;
        } else {
            download_multipart(spec.archive_parts, &cache_dir, &archive_path)?;
        }
    }
    if !archive::verify_sha256(&archive_path, spec.archive_sha256)? {
        fs::remove_file(&archive_path).ok();
        bail!("checksum mismatch for catalog package {}", spec.package_id);
    }

    let staging = base.join(format!(
        ".{}.staging-{}",
        package_key(spec),
        std::process::id()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let result = (|| -> Result<()> {
        archive::extract_tar_zst(&archive_path, &staging)?;
        ensure_toolchain_executables(&staging)?;
        validate_emscripten_payload(&staging)?;
        archive::write_emscripten_config(&staging, "node")?;
        fs::write(staging.join("done.txt"), "ok\n")?;
        write_receipt(&staging, spec, false)?;
        validate_complete_emscripten_install(&staging)?;
        if health_check {
            run_health_checks(&staging)?;
            write_receipt(&staging, spec, true)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        fs::remove_dir_all(&staging).ok();
        return Err(error);
    }
    if destination.exists() {
        if !replace_invalid {
            fs::remove_dir_all(&staging).ok();
            bail!(
                "catalog package {} is present but invalid; run repair",
                spec.package_id
            );
        }
        let quarantine = base.join(format!(
            ".{}.invalid-{}",
            package_key(spec),
            std::process::id()
        ));
        fs::rename(&destination, &quarantine)
            .with_context(|| format!("quarantine invalid toolchain {}", destination.display()))?;
    }
    fs::rename(&staging, &destination)
        .with_context(|| format!("publish toolchain {}", destination.display()))?;
    Ok(destination)
}

fn activate_install(base: &Path, install: &Path, spec: ToolchainSpec) -> Result<()> {
    validate_managed_install(install, spec)?;
    let mut state = read_state(base)?;
    let key = install
        .strip_prefix(base)
        .unwrap_or(install)
        .to_string_lossy()
        .into_owned();
    if state.active.as_deref() != Some(key.as_str()) {
        state.previous_known_good = state.active.take();
        state.active = Some(key);
    }
    state.schema_version = TOOLCHAIN_STATE_SCHEMA;
    write_state(base, &state)
}

fn find_installed_package(base: &Path, package_id: &str, spec: ToolchainSpec) -> Result<PathBuf> {
    for entry in fs::read_dir(base)? {
        let path = entry?.path();
        if path.is_dir()
            && read_receipt(&path).is_ok_and(|receipt| receipt.package_id == package_id)
            && validate_managed_install(&path, spec).is_ok()
        {
            return Ok(path);
        }
    }
    bail!("supported package {package_id} is not installed; run `fastled toolchain install`")
}

fn with_toolchain_lock<T>(base: &Path, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock_path = base.join(".fastled-toolchain.lock");
    let lock = fs::File::create(&lock_path)
        .with_context(|| format!("create toolchain lock {}", lock_path.display()))?;
    lock.lock_exclusive()
        .with_context(|| format!("lock toolchain state {}", lock_path.display()))?;
    let result = action();
    drop(lock);
    result
}

fn run_toolchain_action_locked(
    action: crate::cli::ToolchainAction,
    spec: ToolchainSpec,
    base: &Path,
) -> Result<()> {
    match action {
        crate::cli::ToolchainAction::Status => {
            let active_result = resolve_active_install(base, spec);
            let state = read_state(base)?;
            println!("target: {}/{}", spec.platform, spec.arch);
            println!("release default: {}", spec.package_id);
            println!("active: {}", state.active.as_deref().unwrap_or("none"));
            println!(
                "previous known-good: {}",
                state.previous_known_good.as_deref().unwrap_or("none")
            );
            match active_result {
                Ok(Some(path)) => println!("active state: healthy ({})", path.display()),
                Ok(None) => println!("active state: not installed"),
                Err(error) => println!("active state: invalid ({error:#})"),
            }
        }
        crate::cli::ToolchainAction::Install { package_id } => {
            if package_id
                .as_deref()
                .is_some_and(|id| id != spec.package_id)
            {
                bail!(
                    "package is not in this CLI release catalog: {}",
                    package_id.unwrap()
                );
            }
            let path = install_spec(spec, false, false, false)?;
            println!("installed {} at {}", spec.package_id, path.display());
        }
        crate::cli::ToolchainAction::Activate { package_id } => {
            if package_id != spec.package_id {
                bail!("package is not in this CLI release catalog: {package_id}");
            }
            let path = find_installed_package(base, &package_id, spec)?;
            run_health_checks(&path)?;
            activate_install(base, &path, spec)?;
            println!("activated {}", package_id);
        }
        crate::cli::ToolchainAction::Update => {
            let path = install_spec(spec, true, true, false)?;
            activate_install(base, &path, spec)?;
            println!("updated and activated {}", spec.package_id);
        }
        crate::cli::ToolchainAction::Repair { package_id } => {
            if package_id
                .as_deref()
                .is_some_and(|id| id != spec.package_id)
            {
                bail!(
                    "package is not in this CLI release catalog: {}",
                    package_id.unwrap()
                );
            }
            let path = install_spec(spec, true, true, true)?;
            activate_install(base, &path, spec)?;
            println!("repaired and activated {}", spec.package_id);
        }
        crate::cli::ToolchainAction::Rollback => {
            let mut state = read_state(base)?;
            let previous = state
                .previous_known_good
                .clone()
                .context("no previous known-good toolchain is recorded")?;
            let path = base.join(&previous);
            validate_managed_install(&path, spec)?;
            let old_active = state.active.take();
            state.active = Some(previous);
            state.previous_known_good = old_active;
            write_state(base, &state)?;
            println!("rolled back toolchain");
        }
        crate::cli::ToolchainAction::Prune => {
            let state = read_state(base)?;
            let keep = [
                state.active.as_deref(),
                state.previous_known_good.as_deref(),
            ];
            let mut removed = 0;
            for entry in fs::read_dir(base)? {
                let path = entry?.path();
                let key = path.file_name().and_then(|name| name.to_str());
                if path.is_dir() && key.is_some() && !keep.contains(&key) {
                    fs::remove_dir_all(path)?;
                    removed += 1;
                }
            }
            println!("pruned {removed} inactive toolchain installation(s)");
        }
    }
    Ok(())
}

pub(crate) fn run_toolchain_action(action: crate::cli::ToolchainAction) -> Result<()> {
    let spec = release_default_toolchain()?;
    let base = install_base(spec)?;
    fs::create_dir_all(&base)?;
    with_toolchain_lock(&base, || run_toolchain_action_locked(action, spec, &base))
}

/// Ensure the catalog-selected Emscripten toolchain is active.
///
/// Normal compilation never consults a remote manifest. The only implicit
/// download is a first-run bootstrap when no installation history exists.
pub fn ensure_emscripten_installed() -> Result<PathBuf> {
    let cache = EMSCRIPTEN_INSTALL_CACHE.get_or_init(|| Mutex::new(None));
    let mut cached = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("emscripten install cache lock poisoned"))?;
    if let Some(path) = cached.clone() {
        if validate_complete_emscripten_install(&path).is_ok() {
            return Ok(path);
        }
        *cached = None;
    }
    drop(cached);

    let spec = release_default_toolchain()?;
    let base = install_base(spec)?;
    let installed = with_toolchain_lock(&base, || match resolve_active_install(&base, spec)? {
        Some(path) => Ok(path),
        None if !has_install_history(&base) => {
            let path = install_spec(spec, true, false, false)?;
            activate_install(&base, &path, spec)?;
            Ok(path)
        }
        None => {
            bail!("no supported Emscripten toolchain is active; run `fastled toolchain update`")
        }
    })?;
    let mut cached = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("emscripten install cache lock poisoned"))?;
    *cached = Some(installed.clone());
    Ok(installed)
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
/// Used by `crates/fastled-cli/src/frontend.rs` to bundle frontend assets.
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
// User-facing install flow
// ---------------------------------------------------------------------------

const DEFAULT_INSTALL_EXAMPLE: &str = "wasm";
const AUTO_DEBUG_VSIX_URL: &str =
    "https://github.com/zackees/vscode-auto-debug/releases/latest/download/auto-debug.vsix";

#[derive(Clone, Copy, Debug)]
pub struct InstallOptions {
    pub dry_run: bool,
    pub no_interactive: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct InstallOutcome {
    pub launch_after: bool,
}

fn prompt_yes_no(prompt: &str, default: bool) -> Result<bool> {
    let default_hint = if default { "[Y/n]" } else { "[y/N]" };
    print!("{prompt} {default_hint} ");
    std::io::stdout().flush().context("flush prompt")?;

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("read prompt response")?;
    let trimmed = input.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Ok(default);
    }
    Ok(matches!(trimmed.as_str(), "y" | "yes"))
}

fn command_exists(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn detect_supported_ide() -> Option<(&'static str, &'static str)> {
    if command_exists("code") {
        Some(("code", "VSCode"))
    } else if command_exists("cursor") {
        Some(("cursor", "Cursor"))
    } else {
        None
    }
}

fn find_vscode_project_upward(max_levels: usize) -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    for _ in 0..max_levels {
        let parent = current.parent()?.to_path_buf();
        if parent == current {
            break;
        }
        current = parent;
        if current.join(".vscode").is_dir() {
            return Some(current);
        }
    }
    None
}

fn generate_vscode_project() -> Result<()> {
    let vscode_dir = std::env::current_dir()
        .context("current dir")?
        .join(".vscode");
    fs::create_dir_all(&vscode_dir).with_context(|| format!("create {}", vscode_dir.display()))?;
    println!("Created .vscode directory at {}", vscode_dir.display());
    Ok(())
}

fn validate_vscode_project(no_interactive: bool) -> Result<()> {
    let current_dir = std::env::current_dir().context("current dir")?;
    if current_dir.join(".vscode").is_dir() {
        return Ok(());
    }

    if let Some(parent_path) = find_vscode_project_upward(5) {
        if no_interactive {
            bail!(
                "No .vscode directory found in current directory.\nFound .vscode in parent: {}\nIn non-interactive mode, cannot change directory.\nPlease cd there and run the command again.",
                parent_path.display()
            );
        }
        let use_parent = prompt_yes_no(
            &format!(
                "Found a .vscode project in {}. Install there?",
                parent_path.display()
            ),
            true,
        )?;
        if use_parent {
            std::env::set_current_dir(&parent_path)
                .with_context(|| format!("cd {}", parent_path.display()))?;
            return Ok(());
        }
    }

    if detect_supported_ide().is_none() {
        bail!("No supported IDE found (VSCode or Cursor). Please install VSCode or Cursor first.");
    }

    if no_interactive {
        bail!(
            "No .vscode directory found in current directory or parent directories.\nIn non-interactive mode, cannot create a new project.\nPlease create a .vscode directory or run without --no-interactive."
        );
    }

    println!("No .vscode directory found in current directory or parent directories.");
    if prompt_yes_no(
        "Would you like to generate a VSCode project with FastLED configuration?",
        true,
    )? {
        generate_vscode_project()?;
        return Ok(());
    }

    bail!("installation cancelled");
}

fn detect_fastled_project() -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    let library_json = cwd.join("library.json");
    let Ok(text) = fs::read_to_string(library_json) else {
        return false;
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| value.get("name").and_then(Value::as_str).map(str::to_owned))
        .map(|name| name == "FastLED")
        .unwrap_or(false)
}

fn is_fastled_repository() -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    let required_markers = [
        cwd.join("src").join("FastLED.h"),
        cwd.join("examples").join("Blink").join("Blink.ino"),
        cwd.join("ci").join("ci-compile.py"),
        cwd.join("src").join("platforms"),
        cwd.join("library.json"),
    ];
    if required_markers.iter().any(|path| !path.exists()) {
        return false;
    }

    let Ok(text) = fs::read_to_string(cwd.join("library.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    if value.get("name").and_then(Value::as_str) != Some("FastLED") {
        return false;
    }
    if !value
        .get("repository")
        .and_then(|repo| repo.get("url"))
        .and_then(Value::as_str)
        .map(|url| url.contains("FastLED/FastLED"))
        .unwrap_or(false)
    {
        return false;
    }

    let tests_dir = cwd.join("tests");
    if !tests_dir.is_dir() {
        return false;
    }
    fs::read_dir(tests_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .any(|entry| {
            entry.path().is_file()
                && entry.file_name().to_string_lossy().starts_with("test_")
                && entry.path().extension().and_then(|ext| ext.to_str()) == Some("cpp")
        })
}

fn check_existing_arduino_content() -> bool {
    fn has_ino_file(dir: &Path) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if has_ino_file(&path) {
                    return true;
                }
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("ino") {
                return true;
            }
        }
        false
    }

    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    cwd.join("examples").exists() || has_ino_file(&cwd)
}

pub(crate) fn read_json_file(path: &Path, default: Value) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(default)
}

pub(crate) fn write_json_file(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(value).context("serialize JSON")?;
    text.push('\n');
    fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn update_launch_json_for_arduino() -> Result<()> {
    let cwd = std::env::current_dir().context("current dir")?;
    let launch_json_path = cwd.join(".vscode").join("launch.json");
    let mut data = read_json_file(
        &launch_json_path,
        json!({"version": "0.2.0", "configurations": []}),
    );

    if !data.is_object() {
        data = json!({"version": "0.2.0", "configurations": []});
    }

    let arduino_config = json!({
        "name": "Auto Debug (Smart File Detection)",
        "type": "auto-debug",
        "request": "launch",
        "map": {
            "*.ino": "Arduino: Run .ino with FastLED",
            "*.py": "Python: Current File (UV)"
        }
    });

    let configs = data
        .as_object_mut()
        .expect("launch.json root object")
        .entry("configurations")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !configs.is_array() {
        *configs = Value::Array(Vec::new());
    }
    let configs_array = configs.as_array_mut().expect("configurations array");
    let exists = configs_array.iter().any(|cfg| {
        cfg.get("name").and_then(Value::as_str)
            == arduino_config.get("name").and_then(Value::as_str)
    });
    if !exists {
        configs_array.insert(0, arduino_config);
    }

    write_json_file(&launch_json_path, &data)?;
    println!("Updated {}", launch_json_path.display());
    Ok(())
}

fn generate_fastled_tasks() -> Result<()> {
    let cwd = std::env::current_dir().context("current dir")?;
    let tasks_json_path = cwd.join(".vscode").join("tasks.json");
    let mut data = read_json_file(&tasks_json_path, json!({"version": "2.0.0", "tasks": []}));

    if !data.is_object() {
        data = json!({"version": "2.0.0", "tasks": []});
    }

    let fastled_tasks = vec![
        json!({
            "type": "shell",
            "label": "Run FastLED (Debug)",
            "command": "fastled",
            "args": ["${file}", "--debug"],
            "options": {"cwd": "${workspaceFolder}"},
            "group": {"kind": "build", "isDefault": true},
            "presentation": {
                "echo": true,
                "reveal": "always",
                "focus": true,
                "panel": "new",
                "showReuseMessage": false,
                "clear": true
            },
            "detail": "Run FastLED with debug mode and Tauri visualization",
            "problemMatcher": []
        }),
        json!({
            "type": "shell",
            "label": "Run FastLED (Quick)",
            "command": "fastled",
            "args": ["${file}", "--quick"],
            "options": {"cwd": "${workspaceFolder}"},
            "group": "build",
            "presentation": {
                "echo": true,
                "reveal": "always",
                "focus": true,
                "panel": "new",
                "showReuseMessage": false,
                "clear": true
            },
            "detail": "Run FastLED with quick build mode",
            "problemMatcher": []
        }),
    ];

    let tasks = data
        .as_object_mut()
        .expect("tasks.json root object")
        .entry("tasks")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !tasks.is_array() {
        *tasks = Value::Array(Vec::new());
    }
    let tasks_array = tasks.as_array_mut().expect("tasks array");
    let existing_labels: Vec<String> = tasks_array
        .iter()
        .filter_map(|task| task.get("label").and_then(Value::as_str).map(str::to_owned))
        .collect();

    for task in fastled_tasks {
        let Some(label) = task.get("label").and_then(Value::as_str) else {
            continue;
        };
        if !existing_labels.iter().any(|existing| existing == label) {
            tasks_array.push(task);
        }
    }

    write_json_file(&tasks_json_path, &data)?;
    println!("Updated {}", tasks_json_path.display());
    Ok(())
}

fn fastled_repository_settings() -> Value {
    json!({
        "terminal.integrated.defaultProfile.windows": "Git Bash",
        "terminal.integrated.shellIntegration.enabled": false,
        "terminal.integrated.profiles.windows": {
            "Command Prompt": {"path": "C:\\Windows\\System32\\cmd.exe"},
            "Git Bash": {
                "path": "C:\\Program Files\\Git\\bin\\bash.exe",
                "args": ["--cd=."]
            }
        },
        "files.eol": "\n",
        "files.autoDetectEol": false,
        "files.insertFinalNewline": true,
        "files.trimFinalNewlines": true,
        "editor.tabSize": 4,
        "editor.insertSpaces": true,
        "editor.detectIndentation": true,
        "editor.formatOnSave": false,
        "debug.defaultDebuggerType": "cppdbg",
        "debug.toolBarLocation": "docked",
        "debug.console.fontSize": 14,
        "debug.console.lineHeight": 19,
        "python.defaultInterpreterPath": "uv",
        "python.debugger": "debugpy",
        "[cpp]": {
            "editor.defaultFormatter": "llvm-vs-code-extensions.vscode-clangd",
            "debug.defaultDebuggerType": "cppdbg"
        },
        "[c]": {
            "editor.defaultFormatter": "ms-vscode.cpptools",
            "debug.defaultDebuggerType": "cppdbg"
        },
        "[ino]": {
            "editor.defaultFormatter": "ms-vscode.cpptools",
            "debug.defaultDebuggerType": "cppdbg"
        },
        "clangd.arguments": [
            "--compile-commands-dir=${workspaceFolder}",
            "--clang-tidy",
            "--header-insertion=never",
            "--completion-style=detailed",
            "--function-arg-placeholders=false",
            "--background-index",
            "--pch-storage=memory"
        ],
        "clangd.fallbackFlags": [
            "-std=c++17",
            "-I${workspaceFolder}/src",
            "-I${workspaceFolder}/tests",
            "-Wno-global-constructors"
        ],
        "C_Cpp.intelliSenseEngine": "disabled",
        "C_Cpp.autocomplete": "disabled",
        "C_Cpp.errorSquiggles": "disabled",
        "C_Cpp.suggestSnippets": false,
        "C_Cpp.intelliSenseEngineFallback": "disabled",
        "C_Cpp.autocompleteAddParentheses": false,
        "C_Cpp.formatting": "disabled",
        "C_Cpp.vcpkg.enabled": false,
        "C_Cpp.configurationWarnings": "disabled",
        "C_Cpp.intelliSenseCachePath": "",
        "C_Cpp.intelliSenseCacheSize": 0,
        "C_Cpp.intelliSenseUpdateDelay": 0,
        "C_Cpp.workspaceParsingPriority": "lowest",
        "C_Cpp.disabled": true,
        "files.associations": {
            "*.ino": "cpp",
            "*.h": "cpp",
            "*.hpp": "cpp",
            "*.cpp": "cpp",
            "*.c": "c",
            "*.inc": "cpp",
            "*.tcc": "cpp",
            "*.embeddedhtml": "html",
            "compare": "cpp",
            "type_traits": "cpp",
            "cmath": "cpp",
            "limits": "cpp",
            "iostream": "cpp",
            "random": "cpp",
            "functional": "cpp",
            "bit": "cpp",
            "vector": "cpp",
            "array": "cpp",
            "string": "cpp",
            "memory": "cpp",
            "algorithm": "cpp",
            "iterator": "cpp",
            "utility": "cpp",
            "optional": "cpp",
            "variant": "cpp",
            "numeric": "cpp",
            "chrono": "cpp",
            "thread": "cpp",
            "mutex": "cpp",
            "atomic": "cpp",
            "future": "cpp",
            "condition_variable": "cpp"
        },
        "java.enabled": false,
        "java.jdt.ls.enabled": false,
        "java.compile.nullAnalysis.mode": "disabled",
        "java.configuration.checkProjectSettingsExclusions": false,
        "java.import.gradle.enabled": false,
        "java.import.maven.enabled": false,
        "java.autobuild.enabled": false,
        "java.maxConcurrentBuilds": 0,
        "java.recommendations.enabled": false,
        "java.help.showReleaseNotes": false,
        "redhat.telemetry.enabled": false,
        "java.project.sourcePaths": [],
        "java.project.referencedLibraries": [],
        "files.exclude": {
            "**/.classpath": true,
            "**/.project": true,
            "**/.factorypath": true
        },
        "platformio.disableToolchainAutoInstaller": true,
        "platformio-ide.autoRebuildAutocompleteIndex": false,
        "platformio-ide.activateProjectOnTextEditorChange": false,
        "platformio-ide.autoOpenPlatformIOIniFile": false,
        "platformio-ide.autoPreloadEnvTasks": false,
        "platformio-ide.autoCloseSerialMonitor": false,
        "platformio-ide.disablePIOHomeStartup": true,
        "extensions.ignoreRecommendations": true,
        "editor.semanticTokenColorCustomizations": {
            "rules": {
                "class": "#4EC9B0",
                "struct": "#4EC9B0",
                "type": "#4EC9B0",
                "enum": "#4EC9B0",
                "enumMember": "#B5CEA8",
                "typedef": "#4EC9B0",
                "variable": "#FAFAFA",
                "variable.local": "#FAFAFA",
                "parameter": "#FF8C42",
                "variable.parameter": "#FF8C42",
                "property": "#D197D9",
                "function": "#DCDCAA",
                "method": "#DCDCAA",
                "function.declaration": "#DCDCAA",
                "method.declaration": "#DCDCAA",
                "namespace": "#86C5F7",
                "variable.readonly": {"foreground": "#B5CEA8", "fontStyle": "italic"},
                "variable.defaultLibrary": "#B5CEA8",
                "macro": "#E06C75",
                "string": "#CE9178",
                "number": "#B5CEA8",
                "keyword": "#C586C0",
                "keyword.storage": "#FF79C6",
                "storageClass": "#FF79C6",
                "type.builtin": "#569CD6",
                "keyword.type": "#569CD6",
                "comment": "#6A9955",
                "comment.documentation": "#6A9955"
            }
        },
        "editor.inlayHints.fontColor": "#808080",
        "editor.inlayHints.background": "#3C3C3C20"
    })
}

fn update_vscode_settings_for_fastled() -> Result<()> {
    if !is_fastled_repository() {
        return Ok(());
    }

    let cwd = std::env::current_dir().context("current dir")?;
    let settings_json_path = cwd.join(".vscode").join("settings.json");
    let mut data = read_json_file(&settings_json_path, json!({}));
    if !data.is_object() {
        data = json!({});
    }

    let settings = fastled_repository_settings();
    let target = data.as_object_mut().expect("settings root object");
    let source = settings.as_object().expect("settings object");
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }

    write_json_file(&settings_json_path, &data)?;
    println!(
        "Updated {} with comprehensive FastLED development settings",
        settings_json_path.display()
    );
    Ok(())
}

fn download_to_path(url: &str, dest: &Path) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("build HTTP client")?;
    let bytes = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url} failed"))?
        .error_for_status()
        .with_context(|| format!("download returned error for {url}"))?
        .bytes()
        .context("read response bytes")?;
    fs::write(dest, &bytes).with_context(|| format!("write {}", dest.display()))?;
    Ok(())
}

fn install_auto_debug_extension(dry_run: bool) -> Result<bool> {
    if dry_run {
        println!("[DRY-RUN]: Would download and install Auto Debug extension");
        return Ok(true);
    }

    let Some((ide_command, ide_name)) = detect_supported_ide() else {
        println!("Warning: no supported IDE found (VSCode or Cursor)");
        return Ok(false);
    };

    let temp_dir = tempfile::tempdir().context("create temp dir for extension")?;
    let vsix_path = temp_dir.path().join("auto-debug.vsix");
    println!("Downloading Auto Debug extension...");
    download_to_path(AUTO_DEBUG_VSIX_URL, &vsix_path)?;
    println!("Installing extension in {ide_name}...");

    let status = Command::new(ide_command)
        .args(["--install-extension", &vsix_path.to_string_lossy()])
        .status()
        .with_context(|| format!("launch {ide_command} for extension install"))?;

    if !status.success() {
        println!("Warning: extension installer exited with {}", status);
        return Ok(false);
    }

    println!("Auto Debug extension installed in {ide_name}");
    Ok(true)
}

fn install_default_example() -> Result<bool> {
    let output_dir = PathBuf::from("fastled");
    let repo_root = ensure_fastled_repo(None)?;
    let resolved_ref = crate::project::cached_repo_ref_name(&repo_root);
    let out = crate::project::init_example_from_repo(
        &repo_root,
        DEFAULT_INSTALL_EXAMPLE,
        &output_dir,
        Some(resolved_ref.as_str()),
    )?;
    println!("Installed example at {}", out.display());
    Ok(true)
}

pub fn run_install(options: InstallOptions) -> Result<InstallOutcome> {
    println!("Starting FastLED installation...");
    validate_vscode_project(options.no_interactive)?;

    let is_fastled_project = detect_fastled_project();
    let is_repository = is_fastled_repository();
    if is_fastled_project {
        if is_repository {
            println!("Detected FastLED repository - configuring full development environment");
        } else {
            println!("Detected external FastLED project - configuring Arduino environment");
        }
    } else {
        println!("Detected standard project - configuring basic Arduino environment");
    }

    let should_install_extension = if options.no_interactive {
        println!("Skipping Auto Debug extension installation in non-interactive mode");
        false
    } else if options.dry_run {
        println!("[DRY-RUN]: Simulating Auto Debug extension installation...");
        true
    } else {
        prompt_yes_no(
            "Would you like to install the FastLED auto-debug extension?",
            false,
        )?
    };

    if should_install_extension && !install_auto_debug_extension(options.dry_run)? {
        println!("Warning: Auto Debug extension installation failed, continuing...");
    }

    println!("\nConfiguring VSCode files...");
    update_launch_json_for_arduino()?;
    generate_fastled_tasks()?;

    let mut launch_after = false;
    if !check_existing_arduino_content() {
        if options.no_interactive {
            println!(
                "No Arduino content found. In non-interactive mode, skipping example installation."
            );
        } else if prompt_yes_no(
            &format!(
                "No Arduino content found. Install the default '{}' example?",
                DEFAULT_INSTALL_EXAMPLE
            ),
            true,
        )? {
            if options.dry_run {
                println!(
                    "[DRY-RUN]: Would initialize the default '{}' example",
                    DEFAULT_INSTALL_EXAMPLE
                );
            } else {
                launch_after = install_default_example()?;
            }
        }
    } else {
        println!("Existing Arduino content detected, skipping example installation");
        launch_after = !options.dry_run;
    }

    if is_fastled_project {
        if is_repository {
            println!("\nSetting up FastLED development environment...");
            update_vscode_settings_for_fastled()?;
        } else {
            println!("\nSkipping clangd settings - not in the FastLED repository");
        }
    }

    if options.dry_run {
        println!("\n[DRY-RUN]: Skipping auto-launch");
        launch_after = false;
    }

    println!("\nFastLED installation completed successfully!");
    Ok(InstallOutcome { launch_after })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARWIN_ARM64_MANIFEST: &str = r#"{
  "latest": "releases-d70a5da89b3e673bf6a482724478fc17e81e575e",
  "versions": {
    "releases-d70a5da89b3e673bf6a482724478fc17e81e575e": {
      "version": "releases-d70a5da89b3e673bf6a482724478fc17e81e575e",
      "href": "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/main/assets/emscripten/darwin/arm64/emscripten-releases-d70a5da89b3e673bf6a482724478fc17e81e575e-darwin-arm64.tar.zst",
      "sha256": "6af749f0d44927c7d4c93c9e407e195257ed690b8e3bd3a43d7a3f4badc52082"
    }
  }
}"#;

    const LINUX_X86_64_MANIFEST_WITH_PARTS: &str = r#"{
  "latest": "4.0.21",
  "versions": {
    "4.0.21": {
      "version": "4.0.21",
      "href": "https://raw.githubusercontent.com/zackees/clang-tool-chain-bins/main/assets/emscripten/linux/x86_64/emscripten-4.0.21-linux-x86_64.tar.zst",
      "sha256": "5cd3cbe0316d37c9b39bdc63691c014f136a5d82a9f08ed29bb7ad62f7a83655",
      "parts": [
        {
          "href": "https://raw.githubusercontent.com/zackees/clang-tool-chain-bins/main/assets/emscripten/linux/x86_64/emscripten-4.0.21-linux-x86_64.tar.zst.part-aa",
          "sha256": "e427aee7d1197f59bcbd7a82a581f8d0bdf484ea24036a3c52903bb26cfd4488",
          "size": 99614720
        }
      ]
    }
  }
}"#;

    fn test_spec() -> ToolchainSpec {
        ToolchainSpec {
            platform: "test-platform",
            arch: "test-arch",
            package_id: "4.0.19",
            archive_url: "https://example.com/emscripten.tar.zst",
            archive_sha256: "b19c2e35b863eb17866034f917d7957514645e179e9d22800729b0dcbb2aa2e2",
            archive_parts: &[],
        }
    }

    fn create_valid_emscripten_install(root: &Path) -> PathBuf {
        let spec = test_spec();
        let install = package_dir(root, spec);
        for path in required_emscripten_payload_files(&install) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                if path.ends_with("emscripten-version.txt") {
                    br#"\"4.0.19\"\n"#.as_slice()
                } else {
                    b"tool\n".as_slice()
                },
            )
            .unwrap();
        }
        fs::write(install.join("done.txt"), "ok\n").unwrap();
        write_receipt(&install, spec, true).unwrap();
        install
    }

    #[test]
    fn parses_nested_versions_manifest_without_parts() {
        let manifest: PlatformManifest =
            serde_json::from_str(DARWIN_ARM64_MANIFEST).expect("parse darwin/arm64 manifest");
        assert_eq!(
            manifest.latest,
            "releases-d70a5da89b3e673bf6a482724478fc17e81e575e"
        );
        let entry = manifest
            .versions
            .get(&manifest.latest)
            .expect("entry for latest");
        assert!(entry.href.contains("emscripten-releases-"));
        assert_eq!(
            entry.sha256,
            "6af749f0d44927c7d4c93c9e407e195257ed690b8e3bd3a43d7a3f4badc52082"
        );
        assert!(entry.parts.is_none());
        assert!(multipart_parts(entry).is_none());
    }

    #[test]
    // Regression coverage for issue #194: a valid active install must not
    // fall through to manifest discovery or an implicit replacement.
    fn active_install_is_selected_without_network_or_manifest_state() {
        let temp = tempfile::tempdir().unwrap();
        let spec = test_spec();
        let install = create_valid_emscripten_install(temp.path());
        write_state(
            temp.path(),
            &ActiveToolchainState {
                schema_version: TOOLCHAIN_STATE_SCHEMA,
                active: Some(package_key(spec)),
                previous_known_good: None,
            },
        )
        .unwrap();

        assert_eq!(
            resolve_active_install(temp.path(), spec).unwrap(),
            Some(install)
        );
    }

    #[test]
    fn legacy_marker_migrates_to_receipt_and_active_state() {
        let temp = tempfile::tempdir().unwrap();
        let spec = test_spec();
        let install = temp.path().join(spec.package_id);
        for path in required_emscripten_payload_files(&install) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"tool\n").unwrap();
        }
        fs::write(install.join("done.txt"), "ok\n").unwrap();
        fs::write(temp.path().join(EMSCRIPTEN_VERSION_MARKER), "4.0.19\n").unwrap();

        assert_eq!(
            resolve_active_install(temp.path(), spec).unwrap(),
            Some(install.clone())
        );
        assert!(receipt_path(&install).is_file());
        assert_eq!(
            read_state(temp.path()).unwrap().active,
            Some("4.0.19".to_string())
        );
    }

    #[test]
    fn existing_history_never_bootstraps_implicitly() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("4.0.18")).unwrap();
        assert!(has_install_history(temp.path()));
    }

    #[test]
    fn invalid_active_uses_previous_known_good_without_rewriting_state() {
        let temp = tempfile::tempdir().unwrap();
        let spec = test_spec();
        let install = create_valid_emscripten_install(temp.path());
        let broken = temp.path().join("broken");
        fs::create_dir(&broken).unwrap();
        let state = ActiveToolchainState {
            schema_version: TOOLCHAIN_STATE_SCHEMA,
            active: Some("broken".to_string()),
            previous_known_good: Some(package_key(spec)),
        };
        write_state(temp.path(), &state).unwrap();

        assert_eq!(
            resolve_active_install(temp.path(), spec).unwrap(),
            Some(install)
        );
        assert_eq!(read_state(temp.path()).unwrap(), state);
    }

    #[test]
    fn missing_required_tool_rejects_managed_install() {
        let temp = tempfile::tempdir().unwrap();
        let spec = test_spec();
        let install = create_valid_emscripten_install(temp.path());
        fs::remove_file(install.join(if cfg!(windows) {
            "bin/wasm-ld.exe"
        } else {
            "bin/wasm-ld"
        }))
        .unwrap();

        assert!(validate_complete_emscripten_install(&install).is_err());
        assert!(validate_managed_install(&install, spec).is_err());
    }

    #[test]
    fn parses_manifest_with_multipart_archive_and_extra_size_field() {
        let manifest: PlatformManifest = serde_json::from_str(LINUX_X86_64_MANIFEST_WITH_PARTS)
            .expect("parse linux/x86_64 manifest");
        let entry = manifest.versions.get("4.0.21").expect("entry for 4.0.21");
        let parts = multipart_parts(entry).expect("parts present");
        assert_eq!(parts.len(), 1);
        assert!(parts[0].href.ends_with(".part-aa"));
    }

    #[cfg(unix)]
    #[test]
    fn ensure_toolchain_executables_restores_unix_execute_bits() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("create bin");
        let tool = bin_dir.join("llvm-ar");
        fs::write(&tool, b"#!/bin/sh\n").expect("write tool");

        let emscripten_dir = temp.path().join("emscripten");
        fs::create_dir_all(&emscripten_dir).expect("create emscripten");
        let launcher = emscripten_dir.join("emcc");
        fs::write(&launcher, b"#!/bin/sh\n").expect("write launcher");

        for path in [&tool, &launcher] {
            let mut permissions = fs::metadata(path).expect("metadata").permissions();
            permissions.set_mode(0o644);
            fs::set_permissions(path, permissions).expect("clear executable bit");
        }

        ensure_toolchain_executables(temp.path()).expect("restore executable bits");

        for path in [&tool, &launcher] {
            let mode = fs::metadata(path).expect("metadata").permissions().mode();
            assert_ne!(mode & 0o111, 0);
        }
    }

    /// Regression for issue #111: the win/x86_64 manifest publishes version
    /// entries as siblings of `latest` rather than under a `versions` map.
    /// The CLI must accept that legacy shape.
    #[test]
    fn parses_legacy_flat_manifest_shape() {
        let text = r#"{
            "latest": "4.0.19",
            "4.0.19": {
                "href": "https://example.com/em-4.0.19.tar.zst",
                "sha256": "b19c2e35b863eb17866034f917d7957514645e179e9d22800729b0dcbb2aa2e2"
            }
        }"#;
        let manifest = parse_platform_manifest(text).expect("parse legacy manifest");
        assert_eq!(manifest.latest, "4.0.19");
        let entry = manifest
            .versions
            .get("4.0.19")
            .expect("entry for latest version");
        assert_eq!(entry.sha256.len(), 64);
        assert!(entry.href.ends_with(".tar.zst"));
    }

    #[test]
    fn legacy_manifest_with_no_versions_errors() {
        let text = r#"{ "latest": "4.0.19" }"#;
        assert!(parse_platform_manifest(text).is_err());
    }

    #[test]
    fn legacy_manifest_with_unknown_latest_errors() {
        let text = r#"{
            "latest": "9.9.9",
            "4.0.19": {
                "href": "https://example.com/em.tar.zst",
                "sha256": "abcdef"
            }
        }"#;
        assert!(parse_platform_manifest(text).is_err());
    }
}
