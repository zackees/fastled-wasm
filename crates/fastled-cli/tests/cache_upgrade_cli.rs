use std::fs;
use std::process::{Command, Output};

fn run_fastled(root: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastled"))
        .arg("--version")
        .env("FASTLED_HOME", root)
        .env("FASTLED_MANAGED_RUNTIME", "1")
        .output()
        .expect("run fastled --version")
}

#[test]
fn cli_upgrade_clears_the_shared_cache_exactly_once_per_version() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("cache")).unwrap();
    fs::write(root.join("cache/stale.txt"), "stale").unwrap();
    fs::create_dir_all(root.join("toolchains")).unwrap();
    fs::write(root.join("toolchains/keep.txt"), "keep").unwrap();

    let first = run_fastled(root);

    assert!(first.status.success(), "{first:?}");
    let first_stderr = String::from_utf8(first.stderr).unwrap();
    let expected_warning = format!(
        "warning: FastLED CLI was updated to {}; removing cache at {}",
        env!("CARGO_PKG_VERSION"),
        root.join("cache").display()
    );
    assert!(first_stderr.contains(&expected_warning), "{first_stderr:?}");
    assert!(!root.join("cache").exists());
    assert_eq!(
        fs::read_to_string(root.join("toolchains/keep.txt")).unwrap(),
        "keep"
    );
    assert_eq!(
        fs::read_to_string(
            root.join("state")
                .join(format!("cache-cleared-v{}", env!("CARGO_PKG_VERSION")))
        )
        .unwrap(),
        format!("{}\n", env!("CARGO_PKG_VERSION"))
    );

    fs::create_dir_all(root.join("cache")).unwrap();
    fs::write(root.join("cache/fresh.txt"), "fresh").unwrap();

    let second = run_fastled(root);

    assert!(second.status.success(), "{second:?}");
    let second_stderr = String::from_utf8(second.stderr).unwrap();
    assert!(
        !second_stderr.contains("removing cache"),
        "{second_stderr:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join("cache/fresh.txt")).unwrap(),
        "fresh"
    );
}
