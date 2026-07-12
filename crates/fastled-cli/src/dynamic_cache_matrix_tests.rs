use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::dynamic_cache;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Invocations {
    library_build: usize,
    main_link: usize,
    sketch_compile: usize,
    side_link: usize,
    app_build: usize,
    asset_manifest: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct OutputHashes {
    runtime_js: String,
    runtime_wasm: String,
    sketch_wasm: String,
}

struct FakeRepository {
    _temp: tempfile::TempDir,
    root: PathBuf,
    fastled: PathBuf,
    app: PathBuf,
    mode: String,
    toolchain: String,
    abi: String,
    no_app: bool,
}

impl FakeRepository {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let fastled = root.join("FastLED");
        write(&fastled.join("src/core.cpp"), "core-v1");
        write(&fastled.join("src/core.h"), "header-v1");
        write(
            &fastled.join("src/platforms/wasm/compiler/build_flags.toml"),
            "flags-v1",
        );
        write(
            &fastled.join("src/platforms/wasm/compiler/js_library.js"),
            "js-library-v1",
        );
        write(&fastled.join("meson.build"), "meson-v1");
        write(&fastled.join("ci/meson/wasm/meson.build"), "wasm-meson-v1");

        for (name, source) in [("SketchA", "setup-a"), ("SketchB", "setup-b")] {
            let sketch = root.join(name);
            write(&sketch.join(format!("{name}.ino")), source);
            write(&sketch.join("include/config.h"), "config-v1");
            write(&sketch.join("data/config.json"), "{\"asset\":1}");
        }
        let app = root.join("app");
        write(&app.join("index.ts"), "app-v1");

        Self {
            _temp: temp,
            root,
            fastled,
            app,
            mode: "quick".to_string(),
            toolchain: "emscripten-4.0.19".to_string(),
            abi: "MAIN_MODULE=1;SIDE_MODULE=1;WASM_BIGINT=0".to_string(),
            no_app: false,
        }
    }

    fn sketch(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn runtime_root(&self) -> PathBuf {
        self.fastled
            .join(".build")
            .join("fake-dynamic-runtime-cache")
    }

    fn runtime_fingerprint(&self) -> String {
        let source = dynamic_cache::fingerprint_tree(
            &self.fastled,
            &[
                "src/**/*.cpp",
                "src/**/*.h",
                "src/**/*.toml",
                "src/**/*.js",
                "meson.build",
                "ci/meson/wasm/meson.build",
            ],
            &[".build/**"],
        )
        .unwrap();
        dynamic_cache::fingerprint_values([
            source.as_bytes(),
            self.mode.as_bytes(),
            self.toolchain.as_bytes(),
            self.abi.as_bytes(),
            b"schema=1",
        ])
    }

    fn library_fingerprint(&self) -> String {
        let source = dynamic_cache::fingerprint_tree(
            &self.fastled,
            &[
                "src/**/*.cpp",
                "src/**/*.h",
                "src/**/*.toml",
                "meson.build",
                "ci/meson/wasm/meson.build",
            ],
            &[".build/**"],
        )
        .unwrap();
        dynamic_cache::fingerprint_values([
            source.as_bytes(),
            self.mode.as_bytes(),
            self.toolchain.as_bytes(),
        ])
    }

    fn sketch_fingerprint(&self, name: &str, runtime: &str) -> String {
        let sketch = self.sketch(name);
        let source = dynamic_cache::fingerprint_tree(
            &sketch,
            &["**/*.ino", "**/*.cpp", "**/*.c", "**/*.h", "**/*.hpp"],
            &[".build/**", "fastled_js/**", "data/**"],
        )
        .unwrap();
        dynamic_cache::fingerprint_values([
            source.as_bytes(),
            runtime.as_bytes(),
            self.mode.as_bytes(),
            self.toolchain.as_bytes(),
            self.abi.as_bytes(),
            b"generated-wrapper-v1",
        ])
    }

    fn run(&self, name: &str) -> (Invocations, OutputHashes) {
        let mut calls = Invocations::default();
        let runtime_fingerprint = self.runtime_fingerprint();
        let library_fingerprint = self.library_fingerprint();
        let library_marker = self
            .fastled
            .join(".build")
            .join(format!("fake-library-{}", self.mode));
        if fs::read_to_string(&library_marker).unwrap_or_default() != library_fingerprint {
            calls.library_build += 1;
            write(&library_marker, &library_fingerprint);
        }
        let runtime_root = self.runtime_root();
        let runtime_entry = dynamic_cache::entry_path(&runtime_root, &runtime_fingerprint);
        let _runtime_lock =
            dynamic_cache::CacheLock::acquire(&runtime_root, &runtime_fingerprint).unwrap();
        if dynamic_cache::validate_entry(
            &runtime_entry,
            &runtime_fingerprint,
            &["fastled.js", "fastled.wasm"],
        )
        .is_err()
        {
            calls.main_link += 1;
            let staging = dynamic_cache::staging_dir(&runtime_root, ".fake-runtime-").unwrap();
            write(
                &staging.path().join("fastled.js"),
                &format!("fake loader {runtime_fingerprint}"),
            );
            write_wasm(
                &staging.path().join("fastled.wasm"),
                runtime_fingerprint.as_bytes(),
            );
            dynamic_cache::write_metadata(
                staging.path(),
                &runtime_fingerprint,
                &["fastled.js", "fastled.wasm"],
            )
            .unwrap();
            dynamic_cache::publish_staging(staging, &runtime_entry).unwrap();
        }
        drop(_runtime_lock);

        let sketch_fingerprint = self.sketch_fingerprint(name, &runtime_fingerprint);
        let sketch_root = self.sketch(name).join(".build/fake-sketch-cache");
        let object_entry =
            dynamic_cache::entry_path(&sketch_root.join("objects"), &sketch_fingerprint);
        if dynamic_cache::validate_entry(&object_entry, &sketch_fingerprint, &["sketch.o"]).is_err()
        {
            calls.sketch_compile += 1;
            publish_fake(
                &sketch_root.join("objects"),
                &sketch_fingerprint,
                "sketch.o",
                sketch_fingerprint.as_bytes(),
                false,
            );
        }

        let side_entry = dynamic_cache::entry_path(&sketch_root.join("sides"), &sketch_fingerprint);
        if dynamic_cache::validate_entry(&side_entry, &sketch_fingerprint, &["sketch.wasm"])
            .is_err()
        {
            calls.side_link += 1;
            publish_fake(
                &sketch_root.join("sides"),
                &sketch_fingerprint,
                "sketch.wasm",
                sketch_fingerprint.as_bytes(),
                true,
            );
        }

        let output = self.sketch(name).join("fastled_js");
        fs::create_dir_all(&output).unwrap();
        fs::copy(runtime_entry.join("fastled.js"), output.join("fastled.js")).unwrap();
        fs::copy(
            runtime_entry.join("fastled.wasm"),
            output.join("fastled.wasm"),
        )
        .unwrap();
        fs::copy(side_entry.join("sketch.wasm"), output.join("sketch.wasm")).unwrap();

        let app_fingerprint = dynamic_cache::fingerprint_tree(&self.app, &[], &[]).unwrap();
        let desired_app = format!("{app_fingerprint}:no-app={}", self.no_app);
        let app_marker = output.join(".fake-app-fingerprint");
        if fs::read_to_string(&app_marker).unwrap_or_default() != desired_app {
            calls.app_build += 1;
            write(&app_marker, &desired_app);
        }

        let assets = self.sketch(name).join("data");
        let asset_fingerprint = dynamic_cache::fingerprint_tree(&assets, &[], &[]).unwrap();
        let asset_marker = output.join(".fake-asset-fingerprint");
        if fs::read_to_string(&asset_marker).unwrap_or_default() != asset_fingerprint {
            calls.asset_manifest += 1;
            write(&asset_marker, &asset_fingerprint);
        }

        (
            calls,
            OutputHashes {
                runtime_js: digest(&output.join("fastled.js")),
                runtime_wasm: digest(&output.join("fastled.wasm")),
                sketch_wasm: digest(&output.join("sketch.wasm")),
            },
        )
    }
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn write_wasm(path: &Path, payload: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend_from_slice(payload);
    fs::write(path, bytes).unwrap();
}

fn publish_fake(root: &Path, fingerprint: &str, name: &str, payload: &[u8], wasm: bool) {
    let staging = dynamic_cache::staging_dir(root, ".fake-").unwrap();
    if wasm {
        write_wasm(&staging.path().join(name), payload);
    } else {
        fs::write(staging.path().join(name), payload).unwrap();
    }
    dynamic_cache::write_metadata(staging.path(), fingerprint, &[name]).unwrap();
    dynamic_cache::publish_staging(staging, &dynamic_cache::entry_path(root, fingerprint)).unwrap();
}

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

fn assert_runtime_rebuild(calls: Invocations) {
    assert_eq!(calls.library_build, 1, "library build count: {calls:?}");
    assert_eq!(calls.main_link, 1, "main link count: {calls:?}");
    assert_eq!(calls.sketch_compile, 1, "sketch compile count: {calls:?}");
    assert_eq!(calls.side_link, 1, "side link count: {calls:?}");
}

#[test]
fn cold_noop_and_sketch_only_transitions_use_minimal_commands() {
    let repo = FakeRepository::new();
    let (cold, original) = repo.run("SketchA");
    assert_runtime_rebuild(cold);
    assert_eq!(cold.app_build, 1);
    assert_eq!(cold.asset_manifest, 1);

    let (noop, unchanged) = repo.run("SketchA");
    assert_eq!(noop, Invocations::default());
    assert_eq!(unchanged, original);

    write(&repo.sketch("SketchA").join("SketchA.ino"), "setup-a-v2");
    let (edited, changed) = repo.run("SketchA");
    assert_eq!(edited.library_build, 0);
    assert_eq!(edited.main_link, 0);
    assert_eq!(edited.sketch_compile, 1);
    assert_eq!(edited.side_link, 1);
    assert_eq!(changed.runtime_js, original.runtime_js);
    assert_eq!(changed.runtime_wasm, original.runtime_wasm);
    assert_ne!(changed.sketch_wasm, original.sketch_wasm);
}

#[test]
fn every_runtime_input_class_rebuilds_runtime_and_sketch() {
    let mutations: Vec<(Box<dyn Fn(&mut FakeRepository)>, bool)> = vec![
        (
            Box::new(|repo| write(&repo.fastled.join("src/core.cpp"), "core-v2")),
            true,
        ),
        (
            Box::new(|repo| write(&repo.fastled.join("src/core.h"), "header-v2")),
            true,
        ),
        (
            Box::new(|repo| write(&repo.fastled.join("src/new.cpp"), "new-source")),
            true,
        ),
        (
            Box::new(|repo| fs::remove_file(repo.fastled.join("src/core.cpp")).unwrap()),
            true,
        ),
        (
            Box::new(|repo| {
                fs::rename(
                    repo.fastled.join("src/core.cpp"),
                    repo.fastled.join("src/renamed.cpp"),
                )
                .unwrap()
            }),
            true,
        ),
        (
            Box::new(|repo| {
                write(
                    &repo
                        .fastled
                        .join("src/platforms/wasm/compiler/build_flags.toml"),
                    "flags-v2",
                )
            }),
            true,
        ),
        (
            Box::new(|repo| {
                write(
                    &repo
                        .fastled
                        .join("src/platforms/wasm/compiler/js_library.js"),
                    "js-library-v2",
                )
            }),
            false,
        ),
        (
            Box::new(|repo| repo.toolchain = "emscripten-4.0.20".to_string()),
            true,
        ),
        (
            Box::new(|repo| repo.abi = "MAIN_MODULE=1;SIDE_MODULE=1;WASM_BIGINT=1".to_string()),
            false,
        ),
        (Box::new(|repo| repo.mode = "debug".to_string()), true),
        (Box::new(|repo| repo.mode = "release".to_string()), true),
    ];

    for (mutate, library_rebuild) in mutations {
        let mut repo = FakeRepository::new();
        repo.run("SketchA");
        mutate(&mut repo);
        let (calls, _) = repo.run("SketchA");
        assert_eq!(
            calls.library_build,
            usize::from(library_rebuild),
            "{calls:?}"
        );
        assert_eq!(calls.main_link, 1, "{calls:?}");
        assert_eq!(calls.sketch_compile, 1, "{calls:?}");
        assert_eq!(calls.side_link, 1, "{calls:?}");
    }
}

#[test]
fn app_assets_no_app_and_second_sketch_do_not_relink_runtime() {
    let mut repo = FakeRepository::new();
    let (_, baseline) = repo.run("SketchA");

    write(&repo.app.join("index.ts"), "app-v2");
    let (app, app_hashes) = repo.run("SketchA");
    assert_eq!(app.app_build, 1);
    assert_eq!(app.main_link + app.sketch_compile + app.side_link, 0);
    assert_eq!(app_hashes, baseline);

    write(
        &repo.sketch("SketchA").join("data/config.json"),
        "{\"asset\":2}",
    );
    let (assets, asset_hashes) = repo.run("SketchA");
    assert_eq!(assets.asset_manifest, 1);
    assert_eq!(
        assets.main_link + assets.sketch_compile + assets.side_link,
        0
    );
    assert_eq!(asset_hashes, baseline);

    repo.no_app = true;
    let (no_app, no_app_hashes) = repo.run("SketchA");
    assert_eq!(no_app.app_build, 1);
    assert_eq!(
        no_app.main_link + no_app.sketch_compile + no_app.side_link,
        0
    );
    assert_eq!(no_app_hashes, baseline);

    let (second, second_hashes) = repo.run("SketchB");
    assert_eq!(second.library_build, 0);
    assert_eq!(second.main_link, 0);
    assert_eq!(second.sketch_compile, 1);
    assert_eq!(second.side_link, 1);
    assert_eq!(second_hashes.runtime_js, baseline.runtime_js);
    assert_eq!(second_hashes.runtime_wasm, baseline.runtime_wasm);
}

#[test]
fn touch_is_ignored_but_same_size_preserved_mtime_edit_rebuilds() {
    let repo = FakeRepository::new();
    repo.run("SketchA");
    let source = repo.sketch("SketchA").join("SketchA.ino");
    let original_mtime = source.metadata().unwrap().modified().unwrap();

    let file = fs::OpenOptions::new().write(true).open(&source).unwrap();
    file.set_modified(std::time::SystemTime::now()).unwrap();
    assert_eq!(repo.run("SketchA").0, Invocations::default());

    write(&source, "changed");
    assert_eq!(fs::metadata(&source).unwrap().len(), "setup-a".len() as u64);
    let file = fs::OpenOptions::new().write(true).open(&source).unwrap();
    file.set_modified(original_mtime).unwrap();
    let (calls, _) = repo.run("SketchA");
    assert_eq!(calls.main_link, 0);
    assert_eq!(calls.sketch_compile, 1);
    assert_eq!(calls.side_link, 1);
}

#[test]
fn corrupt_runtime_and_side_artifacts_rebuild_smallest_safe_unit() {
    let repo = FakeRepository::new();
    repo.run("SketchA");
    let runtime = repo.runtime_fingerprint();
    let runtime_entry = dynamic_cache::entry_path(&repo.runtime_root(), &runtime);
    fs::write(runtime_entry.join("cache-metadata.json"), "broken").unwrap();
    let (runtime_calls, _) = repo.run("SketchA");
    assert_eq!(runtime_calls.library_build, 0);
    assert_eq!(runtime_calls.main_link, 1);
    assert_eq!(runtime_calls.sketch_compile, 0);
    assert_eq!(runtime_calls.side_link, 0);

    fs::write(runtime_entry.join("fastled.wasm"), b"\0asm\x01").unwrap();
    let (truncated_runtime, _) = repo.run("SketchA");
    assert_eq!(truncated_runtime.main_link, 1);
    assert_eq!(truncated_runtime.side_link, 0);

    let sketch = repo.sketch_fingerprint("SketchA", &runtime);
    let side_entry = dynamic_cache::entry_path(
        &repo
            .sketch("SketchA")
            .join(".build/fake-sketch-cache/sides"),
        &sketch,
    );
    fs::remove_file(side_entry.join("sketch.wasm")).unwrap();
    let (missing_side, _) = repo.run("SketchA");
    assert_eq!(missing_side.main_link, 0);
    assert_eq!(missing_side.sketch_compile, 0);
    assert_eq!(missing_side.side_link, 1);

    fs::write(side_entry.join("sketch.wasm"), b"\0asm\x01").unwrap();
    let (truncated_side, _) = repo.run("SketchA");
    assert_eq!(truncated_side.main_link, 0);
    assert_eq!(truncated_side.sketch_compile, 0);
    assert_eq!(truncated_side.side_link, 1);
}

#[test]
fn interrupted_new_key_preserves_old_entry_and_concurrent_build_links_once() -> Result<()> {
    let mut repo = FakeRepository::new();
    repo.run("SketchA");
    let old_fingerprint = repo.runtime_fingerprint();
    let old_entry = dynamic_cache::entry_path(&repo.runtime_root(), &old_fingerprint);
    let old_hash = digest(&old_entry.join("fastled.wasm"));

    repo.toolchain = "interrupted-toolchain".to_string();
    let new_fingerprint = repo.runtime_fingerprint();
    dynamic_cache::mark_pending(&repo.runtime_root(), &new_fingerprint, "main-link")?;
    let partial = dynamic_cache::staging_dir(&repo.runtime_root(), ".interrupted-")?;
    write_wasm(&partial.path().join("fastled.wasm"), b"partial");
    std::mem::forget(partial);
    assert_eq!(digest(&old_entry.join("fastled.wasm")), old_hash);
    assert!(dynamic_cache::validate_entry(
        &dynamic_cache::entry_path(&repo.runtime_root(), &new_fingerprint),
        &new_fingerprint,
        &["fastled.js", "fastled.wasm"],
    )
    .is_err());

    let concurrent_root = repo.root.join("concurrent-runtime");
    let fingerprint = "shared-key".to_string();
    let links = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let root = concurrent_root.clone();
        let key = fingerprint.clone();
        let links = Arc::clone(&links);
        handles.push(std::thread::spawn(move || {
            let _lock = dynamic_cache::CacheLock::acquire(&root, &key).unwrap();
            let entry = dynamic_cache::entry_path(&root, &key);
            if dynamic_cache::validate_entry(&entry, &key, &["fastled.js", "fastled.wasm"]).is_err()
            {
                links.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(100));
                let staging = dynamic_cache::staging_dir(&root, ".concurrent-").unwrap();
                write(&staging.path().join("fastled.js"), "loader");
                write_wasm(&staging.path().join("fastled.wasm"), b"runtime");
                dynamic_cache::write_metadata(
                    staging.path(),
                    &key,
                    &["fastled.js", "fastled.wasm"],
                )
                .unwrap();
                dynamic_cache::publish_staging(staging, &entry).unwrap();
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(links.load(Ordering::SeqCst), 1);
    assert!(dynamic_cache::validate_entry(
        &dynamic_cache::entry_path(&concurrent_root, &fingerprint),
        &fingerprint,
        &["fastled.js", "fastled.wasm"],
    )
    .is_ok());
    Ok(())
}
