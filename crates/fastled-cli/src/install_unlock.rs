//! Unlock the installed executable image on Windows.
//!
//! Windows keeps a running executable's file image locked, so `rm -rf venv`
//! (or pip uninstall/reinstall) fails while `fastled` is running from that
//! install location. Borrowing the zccache/soldr trampoline trick: rename our
//! own image to a `.old.<token>` sibling (legal while running), copy it back
//! to the canonical name, and keep running from the renamed file. The canonical
//! path is then a fresh, unlocked file. Stale `.old.*` siblings are swept on
//! every launch; files still locked by a live process simply fail to delete
//! and are retried on a later launch.

#[cfg(windows)]
pub const NO_UNLOCK_ENV_VAR: &str = "FASTLED_NO_UNLOCK";

/// Best-effort: never fails the launch. No-op on non-Windows platforms.
#[cfg(not(windows))]
pub fn unlock_install_exe(_current_exe: &std::path::Path) {}

#[cfg(windows)]
pub fn unlock_install_exe(current_exe: &std::path::Path) {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    if std::env::var_os(NO_UNLOCK_ENV_VAR).is_some() {
        return;
    }

    let Some(parent) = current_exe.parent() else {
        return;
    };
    let Some(file_name) = current_exe.file_name().and_then(|name| name.to_str()) else {
        return;
    };

    sweep_old_siblings(parent, file_name, current_exe);

    // Already running from a renamed trampoline copy; canonical name is free.
    if file_name.contains(".old.") {
        return;
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as u64 ^ (duration.as_secs() << 32))
        .unwrap_or(0);
    let token = u64::from(std::process::id()) ^ nanos;
    let renamed = parent.join(format!("{file_name}.old.{token:x}"));

    if fs::rename(current_exe, &renamed).is_err() {
        return;
    }
    if fs::copy(&renamed, current_exe).is_err() {
        // Restore the canonical name so the install is not left broken.
        let _ = fs::rename(&renamed, current_exe);
    }
}

#[cfg(windows)]
fn sweep_old_siblings(parent: &std::path::Path, file_name: &str, current_exe: &std::path::Path) {
    let canonical_stem = file_name.split(".old.").next().unwrap_or(file_name);
    let prefix = format!("{canonical_stem}.old.");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        let path = entry.path();
        if path == current_exe {
            continue;
        }
        // Locked files (still running) fail here; ignored and retried later.
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    // unlock_install_exe reads a process-global env var; serialize the tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn unlock_creates_old_sibling_and_fresh_canonical_copy() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("myapp.exe");
        fs::write(&exe, b"binary-bytes").expect("write exe");

        unlock_install_exe(&exe);

        let old_files: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("myapp.exe.old.")
            })
            .collect();
        assert_eq!(old_files.len(), 1, "expected exactly one .old sibling");
        assert_eq!(
            fs::read(&exe).expect("read canonical"),
            b"binary-bytes",
            "canonical copy must have identical bytes"
        );
        assert_eq!(
            fs::read(old_files[0].path()).expect("read old"),
            b"binary-bytes"
        );
    }

    #[test]
    fn sweep_removes_stale_old_siblings() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("myapp.exe");
        fs::write(&exe, b"bytes").expect("write exe");
        let stale = dir.path().join("myapp.exe.old.deadbeef");
        fs::write(&stale, b"stale").expect("write stale");

        unlock_install_exe(&exe);

        assert!(!stale.exists(), "stale .old sibling should be removed");
        assert!(exe.exists());
    }

    #[test]
    fn no_unlock_env_var_disables_trampoline() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("myapp.exe");
        fs::write(&exe, b"bytes").expect("write exe");

        std::env::set_var(NO_UNLOCK_ENV_VAR, "1");
        unlock_install_exe(&exe);
        std::env::remove_var(NO_UNLOCK_ENV_VAR);

        let old_count = fs::read_dir(dir.path())
            .expect("read dir")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains(".old."))
            .count();
        assert_eq!(old_count, 0, "opt-out must skip the rename dance");
    }

    #[test]
    fn already_renamed_image_only_sweeps() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        let renamed = dir.path().join("myapp.exe.old.123abc");
        fs::write(&renamed, b"bytes").expect("write renamed");
        let stale = dir.path().join("myapp.exe.old.456def");
        fs::write(&stale, b"stale").expect("write stale");

        unlock_install_exe(&renamed);

        assert!(
            renamed.exists(),
            "the image we are running from must not be renamed again"
        );
        assert!(!stale.exists(), "other stale siblings still get swept");
        assert!(
            !dir.path().join("myapp.exe.old.123abc.old.").exists(),
            "no nested .old chains"
        );
    }

    #[test]
    fn missing_exe_is_tolerated() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("ghost.exe");
        unlock_install_exe(&exe);
        assert!(!exe.exists());
    }
}
