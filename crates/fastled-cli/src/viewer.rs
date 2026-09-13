//! Tauri viewer launch utilities.
//!
//! The viewer is hosted by this same `fastled` executable. The normal CLI
//! process self-spawns with a hidden `--internal-viewer` flag so packaging only
//! needs one binary while the Tauri event loop still runs in its own process.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;

use anyhow::{Context, Result};
use kernal_api::platform::process::{
    spawn_sync, SpawnStdio, SpawnedChild, StdioSource, SyncEnvironment,
};

// ---------------------------------------------------------------------------
// Binary names (platform-aware)
// ---------------------------------------------------------------------------

#[cfg(windows)]
const FASTLED_EXE_NAMES: &[&str] = &["fastled.exe"];
#[cfg(not(windows))]
const FASTLED_EXE_NAMES: &[&str] = &["fastled"];

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Search for the FastLED CLI binary that can host the Tauri viewer.
///
/// Search order:
/// 1. The currently running executable, when it is already `fastled`.
/// 2. Same directory as the currently running executable.
/// 3. `target/debug/` and `target/release/` relative to the workspace root
///    (detected via the executable path heuristic or `CARGO_MANIFEST_DIR`).
/// 4. `target/<arch-triple>/{debug,release}/` for cross-compiled builds.
/// 5. `PATH` lookup via [`Command::new`].
///
/// Returns `None` if the binary cannot be found.
pub fn find_tauri_viewer() -> Option<PathBuf> {
    // 1. The current process is already the CLI binary in normal CLI use.
    if let Ok(exe) = std::env::current_exe() {
        if is_fastled_binary(&exe) {
            return Some(exe);
        }

        // 2. Sibling of the running executable. This covers wheel installs
        // where Python lives next to the bundled native fastled binary.
        if let Some(dir) = exe.parent() {
            for name in FASTLED_EXE_NAMES {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    // 3. Walk up to find a Cargo workspace root, then check `target/debug`
    // and `target/release`.
    if let Some(workspace_root) = find_workspace_root() {
        for profile in &["debug", "release"] {
            for name in FASTLED_EXE_NAMES {
                let candidate = workspace_root.join("target").join(profile).join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        // 4. Scan `target/<arch-triple>/{debug,release}/` for cross-compiled
        // artifacts (e.g. `target/x86_64-pc-windows-msvc/release/`).
        let target_dir = workspace_root.join("target");
        if let Some(candidate) = find_viewer_in_arch_dirs(&target_dir) {
            return Some(candidate);
        }
    }

    // 5. Fall back to PATH lookup.
    for name in FASTLED_EXE_NAMES {
        if is_on_path(name) {
            // Return just the bare name so the OS resolves it through PATH.
            return Some(PathBuf::from(name));
        }
    }

    None
}

/// Scan `<target_dir>/<arch-triple>/{debug,release}/` for the FastLED binary.
/// Skips dotfiles and non-directory entries.
fn find_viewer_in_arch_dirs(target_dir: &std::path::Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(target_dir).ok()?;
    for entry in entries.flatten() {
        let arch_dir = entry.path();
        if !arch_dir.is_dir() {
            continue;
        }
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        for profile in &["debug", "release"] {
            for name in FASTLED_EXE_NAMES {
                let candidate = arch_dir.join(profile).join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Return `true` when a FastLED binary that can host the viewer can be found.
#[inline]
pub fn viewer_available() -> bool {
    find_tauri_viewer().is_some()
}

// ---------------------------------------------------------------------------
// Launch
// ---------------------------------------------------------------------------

/// The kernel owns native containment and process-handle lifetimes.
pub struct ViewerProcess {
    child: SpawnedChild,
}

impl ViewerProcess {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Probe liveness without blocking. Observation errors stop the CLI rather
    /// than allowing it to serve indefinitely after the viewer disappears.
    pub fn is_alive(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
    }
}

fn viewer_command(binary: &Path, url: &str, inject_test_runtime: bool) -> Command {
    let mut command = Command::new(binary);
    command.arg("--internal-viewer").arg(url);
    if inject_test_runtime {
        command.arg("--viewer-inject-test-runtime");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Preserve the former unlabelled process group's discovery policy.
    #[cfg(not(windows))]
    command.env_remove("RUNNING_PROCESS_ORIGINATOR");

    command
}

/// Spawn the Tauri viewer, pointing it at the FastLED HTTP server `url`.
///
/// The viewer is launched without inheriting or creating a terminal window, but
/// it remains contained by this process. Keep the returned [`ViewerProcess`]
/// alive while FastLED is serving; if FastLED exits or is killed, the
/// viewer/WebView2 process tree is torn down too.
///
/// Returns a process handle whose lifetime controls the viewer lifetime.
///
/// Fails loudly when `url` is not an http(s) URL. The viewer must always be
/// pointed at the embedded HTTP server (which serves the loading page and
/// reloads on build completion); pointing it at a directory or `fastled://`
/// path regresses to a dead "Not Found" page (issues #151/#160).
pub fn launch_tauri_viewer(url: &str) -> Result<ViewerProcess> {
    launch_tauri_viewer_with_options(url, false)
}

pub(crate) fn launch_tauri_test_viewer(url: &str) -> Result<ViewerProcess> {
    launch_tauri_viewer_with_options(url, true)
}

fn launch_tauri_viewer_with_options(url: &str, inject_test_runtime: bool) -> Result<ViewerProcess> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        anyhow::bail!(
            "viewer must be launched with an http(s) URL pointing at the FastLED server, got: {url}"
        );
    }

    let binary =
        find_tauri_viewer().context("fastled binary not found; cannot launch Tauri viewer")?;

    let mut command = viewer_command(&binary, url, inject_test_runtime);
    let stdio = SpawnStdio {
        stdin: StdioSource::Null,
        stdout: StdioSource::Null,
        stderr: StdioSource::Null,
        show_console: false,
        ..SpawnStdio::default()
    };
    let child = spawn_sync(&mut command, stdio, SyncEnvironment::Inherit)
        .with_context(|| format!("failed to spawn FastLED viewer from '{}'", binary.display()))?;
    Ok(ViewerProcess { child })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Walk up the filesystem from the current executable or `CARGO_MANIFEST_DIR`
/// until a directory containing a `Cargo.toml` file is found. This is a
/// heuristic to locate the workspace root during development; it gracefully
/// returns `None` in production installs where there is no `Cargo.toml`.
fn find_workspace_root() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if let Some(root) = find_workspace_root_from(parent) {
                return Some(root);
            }
        }
    }

    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        if let Some(root) = find_workspace_root_from(Path::new(manifest_dir)) {
            return Some(root);
        }
    }

    None
}

fn find_workspace_root_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();

    for _ in 0..10 {
        if dir.join("Cargo.toml").is_file() {
            return Some(dir.clone());
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    None
}

/// Check whether `name` resolves on PATH by attempting a no-op invocation.
fn is_on_path(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn is_fastled_binary(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    FASTLED_EXE_NAMES
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_viewer_available_does_not_panic() {
        // We don't assert a specific value; the binary may or may not be
        // present in CI. We only verify no panic occurs.
        let _ = viewer_available();
    }

    #[test]
    fn test_find_tauri_viewer_returns_option() {
        // Same as above: just confirm the function runs without panicking.
        let result = find_tauri_viewer();
        if let Some(p) = result {
            assert!(p.file_name().is_some(), "expected a non-empty path");
        }
    }

    #[test]
    fn test_find_workspace_root_does_not_panic() {
        let _ = find_workspace_root();
    }

    #[test]
    fn test_find_viewer_in_arch_dirs_finds_binary() {
        // Set up a fake target tree:
        //   <tmp>/target/x86_64-pc-windows-msvc/release/fastled[.exe]
        let tmp = TempDir::new().expect("tempdir");
        let target = tmp.path().join("target");
        let arch_dir = target.join("x86_64-pc-windows-msvc").join("release");
        fs::create_dir_all(&arch_dir).expect("mkdir arch_dir");
        let viewer_path = arch_dir.join(FASTLED_EXE_NAMES[0]);
        fs::write(&viewer_path, b"fake binary").expect("write fake viewer");

        let found = find_viewer_in_arch_dirs(&target).expect("expected to find viewer");
        assert_eq!(found, viewer_path);
    }

    #[test]
    fn test_find_viewer_in_arch_dirs_skips_dotfiles() {
        // A hidden `.cache` dir should not be scanned.
        let tmp = TempDir::new().expect("tempdir");
        let target = tmp.path().join("target");
        let hidden = target.join(".cache").join("debug");
        fs::create_dir_all(&hidden).expect("mkdir hidden");
        fs::write(hidden.join(FASTLED_EXE_NAMES[0]), b"fake").expect("write fake");

        assert!(find_viewer_in_arch_dirs(&target).is_none());
    }

    #[test]
    fn test_find_viewer_in_arch_dirs_returns_none_for_empty_tree() {
        let tmp = TempDir::new().expect("tempdir");
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).expect("mkdir target");
        assert!(find_viewer_in_arch_dirs(&target).is_none());
    }

    #[test]
    fn test_find_viewer_in_arch_dirs_missing_target() {
        let tmp = TempDir::new().expect("tempdir");
        let missing = tmp.path().join("does-not-exist");
        assert!(find_viewer_in_arch_dirs(&missing).is_none());
    }

    #[test]
    fn test_viewer_command_uses_internal_viewer_flag() {
        let command = viewer_command(
            Path::new(FASTLED_EXE_NAMES[0]),
            "http://127.0.0.1:8089",
            false,
        );
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, vec!["--internal-viewer", "http://127.0.0.1:8089"]);
    }

    #[test]
    fn test_viewer_command_can_inject_test_runtime() {
        let command = viewer_command(
            Path::new(FASTLED_EXE_NAMES[0]),
            "http://127.0.0.1:8089",
            true,
        );
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            vec![
                "--internal-viewer",
                "http://127.0.0.1:8089",
                "--viewer-inject-test-runtime"
            ]
        );
    }

    #[test]
    fn test_launch_tauri_viewer_rejects_directory_path() {
        // Regression gate for #151/#160: the viewer must never be pointed at
        // a filesystem path; it must load the FastLED HTTP server URL.
        match launch_tauri_viewer("examples/TwinkleFox/fastled_js") {
            Ok(_) => panic!("directory path must be rejected"),
            Err(err) => assert!(err.to_string().contains("http(s) URL"), "got: {err}"),
        }
    }

    #[test]
    fn test_launch_tauri_viewer_rejects_fastled_protocol() {
        match launch_tauri_viewer("fastled://localhost/index.html") {
            Ok(_) => panic!("fastled:// protocol must be rejected"),
            Err(err) => assert!(err.to_string().contains("http(s) URL"), "got: {err}"),
        }
    }

    #[test]
    fn test_viewer_process_is_alive_alive_to_dead() {
        #[cfg(not(windows))]
        let mut command = Command::new("sleep");
        #[cfg(not(windows))]
        command.arg("30");
        #[cfg(windows)]
        let mut command = Command::new("ping");
        #[cfg(windows)]
        command.args(["-n", "30", "127.0.0.1"]);
        let stdio = SpawnStdio {
            stdin: StdioSource::Null,
            stdout: StdioSource::Null,
            stderr: StdioSource::Null,
            ..SpawnStdio::default()
        };
        let child =
            spawn_sync(&mut command, stdio, SyncEnvironment::Inherit).expect("spawn viewer child");
        let mut viewer = ViewerProcess { child };

        assert!(
            viewer.is_alive(),
            "expected freshly spawned process to be alive"
        );

        viewer.child.kill().expect("kill sleep");

        // The kill is asynchronous from the probe's perspective; poll briefly.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while viewer.is_alive() {
            assert!(
                std::time::Instant::now() < deadline,
                "expected killed process to be observed dead within 5s"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
