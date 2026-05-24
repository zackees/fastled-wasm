//! Tauri viewer launch utilities.
//!
//! The viewer is hosted by this same `fastled` executable. The normal CLI
//! process self-spawns with a hidden `--internal-viewer` flag so packaging only
//! needs one binary while the Tauri event loop still runs in its own process.

use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(any(not(windows), test))]
use std::process::Stdio;

use anyhow::{Context, Result};
#[cfg(not(windows))]
use running_process::{ContainedProcessGroup, SpawnStdio, SpawnedChild, StdioSource};

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

#[cfg(windows)]
const VIEWER_CREATION_FLAGS: u32 = 0x0800_0000 | 0x0000_0200; // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP

#[cfg(windows)]
pub struct ViewerProcess {
    job: windows_sys::Win32::Foundation::HANDLE,
    process: windows_sys::Win32::Foundation::HANDLE,
    thread: windows_sys::Win32::Foundation::HANDLE,
    pid: u32,
}

#[cfg(not(windows))]
pub struct ViewerProcess {
    _group: ContainedProcessGroup,
    child: SpawnedChild,
}

impl ViewerProcess {
    #[cfg(windows)]
    pub fn pid(&self) -> u32 {
        self.pid
    }

    #[cfg(not(windows))]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

#[cfg(windows)]
impl Drop for ViewerProcess {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.thread);
            windows_sys::Win32::Foundation::CloseHandle(self.process);
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

#[cfg(any(not(windows), test))]
fn viewer_command(binary: &Path, frontend_dir: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("--internal-viewer")
        .arg(frontend_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(VIEWER_CREATION_FLAGS);
    }

    command
}

#[cfg(windows)]
fn quote_windows_arg(arg: &std::ffi::OsStr) -> String {
    let text = arg.to_string_lossy();
    if !text.is_empty()
        && !text
            .chars()
            .any(|ch| matches!(ch, ' ' | '\t' | '"' | '\n' | '\r'))
    {
        return text.into_owned();
    }

    let mut quoted = String::from("\"");
    let mut backslashes = 0usize;
    for ch in text.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(windows)]
fn viewer_command_line(binary: &Path, frontend_dir: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    let args = [
        binary.as_os_str(),
        std::ffi::OsStr::new("--internal-viewer"),
        frontend_dir.as_os_str(),
    ];
    let command_line = args
        .iter()
        .map(|arg| quote_windows_arg(arg))
        .collect::<Vec<_>>()
        .join(" ");
    std::ffi::OsStr::new(&command_line)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn spawn_hidden_viewer(binary: &Path, frontend_dir: &Path) -> Result<ViewerProcess> {
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{CloseHandle, FALSE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, ResumeThread, TerminateProcess, CREATE_SUSPENDED, PROCESS_INFORMATION,
        STARTF_USESHOWWINDOW, STARTUPINFOW,
    };

    let job = unsafe { CreateJobObjectW(null_mut(), null()) };
    if job.is_null() {
        return Err(std::io::Error::last_os_error()).context("failed to create viewer job object");
    }

    let mut job_info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    job_info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let set_job_ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&mut job_info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if set_job_ok == FALSE {
        let err = std::io::Error::last_os_error();
        unsafe {
            CloseHandle(job);
        }
        return Err(err).context("failed to configure viewer job object");
    }

    let mut startup_info: STARTUPINFOW = unsafe { zeroed() };
    startup_info.cb = size_of::<STARTUPINFOW>() as u32;
    startup_info.dwFlags = STARTF_USESHOWWINDOW;
    startup_info.wShowWindow = 0; // SW_HIDE

    let mut process_info: PROCESS_INFORMATION = unsafe { zeroed() };
    let mut command_line = viewer_command_line(binary, frontend_dir);
    let flags = VIEWER_CREATION_FLAGS | CREATE_SUSPENDED;
    let create_ok = unsafe {
        CreateProcessW(
            null(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            FALSE,
            flags,
            null(),
            null(),
            &startup_info,
            &mut process_info,
        )
    };
    if create_ok == FALSE {
        let err = std::io::Error::last_os_error();
        unsafe {
            CloseHandle(job);
        }
        return Err(err).with_context(|| {
            format!(
                "failed to create FastLED viewer from '{}'",
                binary.display()
            )
        });
    }

    let assign_ok = unsafe { AssignProcessToJobObject(job, process_info.hProcess) };
    if assign_ok == FALSE {
        let err = std::io::Error::last_os_error();
        unsafe {
            TerminateProcess(process_info.hProcess, 1);
            CloseHandle(process_info.hThread);
            CloseHandle(process_info.hProcess);
            CloseHandle(job);
        }
        return Err(err).context("failed to assign viewer to job object");
    }

    let resume_result = unsafe { ResumeThread(process_info.hThread) };
    if resume_result == u32::MAX {
        let err = std::io::Error::last_os_error();
        unsafe {
            CloseHandle(process_info.hThread);
            CloseHandle(process_info.hProcess);
            CloseHandle(job);
        }
        return Err(err).context("failed to resume viewer process");
    }

    Ok(ViewerProcess {
        job,
        process: process_info.hProcess,
        thread: process_info.hThread,
        pid: process_info.dwProcessId,
    })
}

/// Spawn the Tauri viewer, pointing it at `frontend_dir`.
///
/// The viewer is launched without inheriting or creating a terminal window, but
/// it remains contained by this process. Keep the returned [`ViewerProcess`]
/// alive while FastLED is serving; if FastLED exits or is killed, the
/// viewer/WebView2 process tree is torn down too.
///
/// Returns a process handle whose lifetime controls the viewer lifetime.
pub fn launch_tauri_viewer(frontend_dir: &std::path::Path) -> Result<ViewerProcess> {
    let binary =
        find_tauri_viewer().context("fastled binary not found; cannot launch Tauri viewer")?;

    #[cfg(windows)]
    {
        spawn_hidden_viewer(&binary, frontend_dir)
    }

    #[cfg(not(windows))]
    {
        let group =
            ContainedProcessGroup::new().context("failed to create viewer process group")?;
        let mut command = viewer_command(&binary, frontend_dir);
        let stdio = SpawnStdio {
            stdin: StdioSource::Null,
            stdout: StdioSource::Null,
            stderr: StdioSource::Null,
            ..SpawnStdio::default()
        };
        let child = group.spawn(&mut command, stdio).with_context(|| {
            format!("failed to spawn FastLED viewer from '{}'", binary.display())
        })?;

        Ok(ViewerProcess {
            _group: group,
            child,
        })
    }
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
        let command = viewer_command(Path::new(FASTLED_EXE_NAMES[0]), Path::new("out"));
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, vec!["--internal-viewer", "out"]);
    }

    #[test]
    #[cfg(windows)]
    fn test_viewer_uses_hidden_process_creation_flags() {
        assert_eq!(VIEWER_CREATION_FLAGS & 0x0800_0000, 0x0800_0000);
        assert_eq!(VIEWER_CREATION_FLAGS & 0x0000_0200, 0x0000_0200);
    }
}
