//! Emit clangd / VS Code configuration for a sketch directory so that
//! "Go to Definition" works for WASM sketches (Refs #177).
//!
//! Generation is opt-in (Refs #179): after a successful WASM build run with
//! `fastled --clangd`, or via the standalone `fastled --write-clangd [DIR]`,
//! the sketch directory receives:
//!
//! - `compile_commands.json` — exact compile arguments (bundled clang++,
//!   `--target=wasm32-unknown-emscripten`, `--sysroot`, includes, defines)
//!   for both the `.ino` and the generated wrapper `.cpp`.
//! - `.clangd` — anchors the compilation database and adds fallback flags
//!   for headers opened standalone.
//! - `.vscode/settings.json` — points the clangd extension at the bundled
//!   clang driver via `--query-driver` and disables the Microsoft C/C++
//!   IntelliSense engine which fights clangd.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use crate::install;
use crate::path::NormalizedPath;
use crate::wasm_build::BuildMode;

/// Inputs required to emit the clangd configuration for one sketch.
pub struct ClangdConfigInputs {
    /// The sketch directory (where the three files are written).
    pub sketch_dir: NormalizedPath,
    /// FastLED checkout, e.g. `~/.fastled/cache/fastled-<ref>/`.
    pub fastled_dir: NormalizedPath,
    /// Emscripten install root, e.g.
    /// `~/.fastled/toolchains/emscripten/<platform>/<arch>/<version>/`.
    pub emsdk_install_dir: NormalizedPath,
    /// Path to the emcc wrapper (informational; kept for parity with the
    /// build's `ToolPaths`).
    pub tools_emcc_path: NormalizedPath,
    /// Path to the em++ wrapper (informational).
    pub tools_empp_path: NormalizedPath,
    /// The generated wrapper `.cpp` that is actually compiled.
    pub wrapper_source: NormalizedPath,
    /// Every top-level sketch `.ino` tab, so clangd sees each user buffer as
    /// a translation unit with the same generated declaration prelude.
    pub ino_files: Vec<NormalizedPath>,
    /// The generated Arduino declaration prelude force-included for raw `.ino`
    /// buffers. It is shared by clangd and Microsoft C/C++ IntelliSense.
    pub prototype_header: NormalizedPath,
    /// Result of `get_sketch_compile_flags()` for the active build mode.
    pub compile_flags: Vec<String>,
    /// The active build mode.
    pub build_mode: BuildMode,
}

/// Convert a path to an absolute, forward-slash string with no Windows
/// `\\?\` long-path prefix. clangd accepts forward slashes on all platforms,
/// and stale `\\?\` prefixes break clangd on Windows.
fn display_path(path: &Path) -> String {
    crate::path::canonicalize_normalized(path)
        .into_path_buf()
        .to_string_lossy()
        .replace('\\', "/")
}

/// Path to the bundled clang++ binary inside the emscripten install.
/// clangd parses `arguments[0]` to find builtin include dirs; the em++
/// wrapper is a Python script, not a clang binary, so it cannot be used.
fn bundled_clangxx(emsdk_install_dir: &Path) -> NormalizedPath {
    let name = if cfg!(windows) {
        "clang++.exe"
    } else {
        "clang++"
    };
    NormalizedPath::new(emsdk_install_dir.join("bin").join(name))
}

/// Parse `(major, minor)` from the emsdk install dir's version path
/// component (e.g. `.../emscripten/win/x86_64/4.0.19` -> `(4, 0)`).
fn emscripten_version(emsdk_install_dir: &Path) -> Option<(u32, u32)> {
    let name = emsdk_install_dir.file_name()?.to_str()?;
    let mut parts = name.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|m| m.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

/// Drop flags that clangd's clang does not understand or that are
/// stale-prone for an index-only configuration:
///
/// - `-sFOO[=...]` emscripten linker-style settings (note: `-std=...`,
///   `-shared` etc. have a lowercase letter after `-s` and are kept),
/// - `-flto=...` / `-flto`,
/// - `-fmodule-file=...` (PCM path is build-mode dependent and stale-prone),
/// - `-Xclang` plus its argument,
/// - `-fmodules-codegen`.
fn filter_compile_flags(flags: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(flags.len());
    let mut skip_next = false;
    for flag in flags {
        if skip_next {
            skip_next = false;
            continue;
        }
        if flag == "-s" || flag == "-Xclang" {
            skip_next = true;
            continue;
        }
        let em_setting = flag
            .strip_prefix("-s")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_uppercase());
        if em_setting
            || flag == "-flto"
            || flag.starts_with("-flto=")
            || flag.starts_with("-fmodule-file=")
            || flag == "-fmodules-codegen"
        {
            continue;
        }
        out.push(flag.clone());
    }
    out
}

struct ResolvedPaths {
    sketch_dir: String,
    fastled_dir: String,
    clangxx: String,
    sysroot: String,
    libcxx_include: String,
    sysroot_include: String,
    system_include: String,
    fastled_src: String,
    fastled_wasm_compiler: String,
}

fn resolve_paths(inputs: &ClangdConfigInputs) -> ResolvedPaths {
    let emsdk = crate::path::canonicalize_normalized(&inputs.emsdk_install_dir);
    let fastled_dir = crate::path::canonicalize_normalized(&inputs.fastled_dir);
    let sysroot = emsdk.join("emscripten").join("cache").join("sysroot");
    ResolvedPaths {
        sketch_dir: display_path(&inputs.sketch_dir),
        fastled_dir: fastled_dir.to_string_lossy().replace('\\', "/"),
        clangxx: bundled_clangxx(&emsdk).to_string_lossy().replace('\\', "/"),
        sysroot: sysroot.to_string_lossy().replace('\\', "/"),
        libcxx_include: sysroot
            .join("include")
            .join("c++")
            .join("v1")
            .to_string_lossy()
            .replace('\\', "/"),
        sysroot_include: sysroot.join("include").to_string_lossy().replace('\\', "/"),
        system_include: emsdk
            .join("emscripten")
            .join("system")
            .join("include")
            .to_string_lossy()
            .replace('\\', "/"),
        fastled_src: fastled_dir.join("src").to_string_lossy().replace('\\', "/"),
        fastled_wasm_compiler: fastled_dir
            .join("src")
            .join("platforms")
            .join("wasm")
            .join("compiler")
            .to_string_lossy()
            .replace('\\', "/"),
    }
}

/// Build the `arguments` array of one `compile_commands.json` entry.
fn compile_arguments(
    inputs: &ClangdConfigInputs,
    paths: &ResolvedPaths,
    file: &str,
    force_cpp: bool,
    force_include: Option<&str>,
) -> Vec<String> {
    let mut args = vec![paths.clangxx.clone()];
    if force_cpp {
        // The `.ino` extension is unknown to clang; force C++.
        args.push("-x".to_string());
        args.push("c++".to_string());
    }
    if let Some(header) = force_include {
        args.push("-include".to_string());
        args.push(header.to_string());
    }
    args.extend([
        "-c".to_string(),
        file.to_string(),
        "-o".to_string(),
        format!("{}/.build/wasm/clangd_stub.o", paths.sketch_dir),
        "--target=wasm32-unknown-emscripten".to_string(),
        format!("--sysroot={}", paths.sysroot),
        "-isystem".to_string(),
        paths.libcxx_include.clone(),
        "-isystem".to_string(),
        paths.sysroot_include.clone(),
        "-isystem".to_string(),
        paths.system_include.clone(),
        "-I".to_string(),
        paths.fastled_src.clone(),
        "-I".to_string(),
        paths.fastled_wasm_compiler.clone(),
        "-I".to_string(),
        paths.sketch_dir.clone(),
        "-D__EMSCRIPTEN__=1".to_string(),
    ]);
    if let Some((major, minor)) = emscripten_version(&inputs.emsdk_install_dir) {
        args.push(format!("-D__EMSCRIPTEN_major__={major}"));
        args.push(format!("-D__EMSCRIPTEN_minor__={minor}"));
    }
    args.extend(filter_compile_flags(&inputs.compile_flags));
    args
}

fn write_compile_commands(inputs: &ClangdConfigInputs, paths: &ResolvedPaths) -> Result<()> {
    let wrapper = display_path(&inputs.wrapper_source);
    let prelude = display_path(&inputs.prototype_header);
    // `directory` must be the FastLED checkout because the real build runs
    // with `current_dir(fastled_dir)`.
    let mut entries = inputs
        .ino_files
        .iter()
        .map(|ino_file| {
            let ino = display_path(ino_file);
            json!({
                "directory": paths.fastled_dir,
                "file": ino,
                "arguments": compile_arguments(inputs, paths, &ino, true, Some(&prelude)),
            })
        })
        .collect::<Vec<_>>();
    entries.push(json!({
        "directory": paths.fastled_dir,
        "file": wrapper,
        "arguments": compile_arguments(inputs, paths, &wrapper, false, None),
    }));
    install::write_json_file(
        &inputs.sketch_dir.join("compile_commands.json"),
        &serde_json::Value::Array(entries),
    )
}

fn write_clangd_file(inputs: &ClangdConfigInputs, paths: &ResolvedPaths) -> Result<()> {
    let content = format!(
        "# Generated by `fastled` (build mode: {mode}).\n\
         # Real build driver: {empp} (emcc: {emcc})\n\
         CompileFlags:\n\
         \x20 CompilationDatabase: .\n\
         \x20 Add:\n\
         \x20   - --target=wasm32-unknown-emscripten\n\
         \x20   - --sysroot={sysroot}\n\
         \x20   - -isystem{libcxx}\n\
         \x20   - -I{fastled_src}\n\
         \x20   - -I{fastled_wasm_compiler}\n\
         \x20   - -D__EMSCRIPTEN__=1\n\
         \x20   - -DSKETCH_COMPILE=1\n\
         \x20 Remove:\n\
         \x20   - -fmodule-file=*\n\
         \x20   - -fmodules-codegen\n\
         \x20   - -Xclang\n",
        mode = inputs.build_mode.as_str(),
        empp = display_path(&inputs.tools_empp_path),
        emcc = display_path(&inputs.tools_emcc_path),
        sysroot = paths.sysroot,
        libcxx = paths.libcxx_include,
        fastled_src = paths.fastled_src,
        fastled_wasm_compiler = paths.fastled_wasm_compiler,
    );
    let path = inputs.sketch_dir.join(".clangd");
    std::fs::write(&path, content).with_context(|| format!("write {}", path.display()))
}

fn write_vscode_settings(paths: &ResolvedPaths, sketch_dir: &Path) -> Result<()> {
    let settings_path = sketch_dir.join(".vscode").join("settings.json");
    let mut settings = install::read_json_file(&settings_path, json!({}));
    if !settings.is_object() {
        settings = json!({});
    }
    let object = settings.as_object_mut().expect("settings.json root object");

    object.insert(
        "clangd.arguments".to_string(),
        json!([
            "--compile-commands-dir=${workspaceFolder}",
            format!("--query-driver={}", paths.clangxx),
            "--background-index",
            "--header-insertion=never",
            "--completion-style=detailed",
            "--pch-storage=memory",
        ]),
    );
    object.insert(
        "clangd.fallbackFlags".to_string(),
        json!([
            "-std=c++20",
            "--target=wasm32-unknown-emscripten",
            format!("-I{}", paths.fastled_src),
            format!("-I{}", paths.fastled_wasm_compiler),
            format!("-isystem{}", paths.libcxx_include),
            "-D__EMSCRIPTEN__=1",
        ]),
    );
    let associations = object
        .entry("files.associations")
        .or_insert_with(|| json!({}));
    if !associations.is_object() {
        *associations = json!({});
    }
    associations
        .as_object_mut()
        .expect("files.associations object")
        .insert("*.ino".to_string(), json!("cpp"));

    install::write_json_file(&settings_path, &settings)
}

/// Emit the same compile database and forced declaration prelude for the
/// Microsoft C/C++ extension. #203 decides which engine is active; neither
/// engine gets a different Arduino sketch model.
fn write_cpptools_config(inputs: &ClangdConfigInputs, paths: &ResolvedPaths) -> Result<()> {
    let path = inputs
        .sketch_dir
        .join(".vscode")
        .join("c_cpp_properties.json");
    let mut properties = install::read_json_file(&path, json!({}));
    if !properties.is_object() {
        properties = json!({});
    }
    let root = properties.as_object_mut().expect("properties root object");
    root.insert(
        "configurations".to_string(),
        json!([{
            "name": "FastLED WASM",
            "compilerPath": paths.clangxx,
            "compileCommands": "${workspaceFolder}/compile_commands.json",
            "cppStandard": "c++20",
            "includePath": [paths.sketch_dir, paths.fastled_src, paths.fastled_wasm_compiler],
            "forcedInclude": [display_path(&inputs.prototype_header)],
        }]),
    );
    install::write_json_file(&path, &properties)
}

/// Write `compile_commands.json`, `.clangd`, and `.vscode/settings.json`
/// into the sketch directory. All emitted paths are absolute, use forward
/// slashes, and carry no Windows `\\?\` prefix.
pub fn write_clangd_config(inputs: &ClangdConfigInputs) -> Result<()> {
    let paths = resolve_paths(inputs);
    write_compile_commands(inputs, &paths)?;
    write_clangd_file(inputs, &paths)?;
    write_vscode_settings(&paths, &inputs.sketch_dir)?;
    write_cpptools_config(inputs, &paths)?;
    Ok(())
}

/// Handler for `fastled --write-clangd[=DIR]`: regenerate the clangd
/// configuration for a sketch directory without compiling. Ensures the
/// emscripten toolchain and the FastLED repo are present (downloading them
/// if necessary), materializes the wrapper `.cpp`, and writes the config.
pub fn run_write_clangd(dir_arg: &str) -> Result<()> {
    let sketch_dir = if dir_arg == "__cwd__" {
        NormalizedPath::new(std::env::current_dir().context("resolve current directory")?)
    } else {
        NormalizedPath::new(dir_arg)
    };
    let sketch_dir = crate::path::canonicalize_normalized(&sketch_dir);
    if !sketch_dir.is_dir() {
        anyhow::bail!("sketch directory does not exist: {}", sketch_dir.display());
    }
    let emsdk_install_dir = NormalizedPath::new(install::ensure_emscripten_installed()?);
    let tools_emcc_path = emsdk_install_dir.join("emscripten").join("emcc.py");
    let tools_empp_path = emsdk_install_dir.join("emscripten").join("em++.py");

    let fastled_dir = NormalizedPath::new(crate::wasm_build::resolve_fastled_dir_for_sketch(
        &sketch_dir,
    )?);
    let (example_name, example_dir, _is_in_tree) =
        crate::wasm_build::resolve_example_name(&sketch_dir, &fastled_dir);
    let example_dir = NormalizedPath::new(example_dir);
    let sketch_cache = crate::wasm_build::sketch_cache_dir(&example_dir);
    let (snapshot, prototype_header) =
        crate::sketch_preprocessor::write_disk_snapshot(&example_dir)?;
    let wrapper_source = NormalizedPath::new(crate::wasm_build::create_wrapper(
        &example_dir,
        &example_name,
        &sketch_cache,
    )?);

    let build_mode = BuildMode::Quick;
    let compile_flags =
        crate::wasm_build::get_sketch_compile_flags(&fastled_dir, build_mode, None)?;

    write_clangd_config(&ClangdConfigInputs {
        sketch_dir: example_dir.clone(),
        fastled_dir,
        emsdk_install_dir,
        tools_emcc_path,
        tools_empp_path,
        wrapper_source,
        ino_files: snapshot
            .documents
            .iter()
            .map(|document| NormalizedPath::new(&document.path))
            .collect(),
        prototype_header: NormalizedPath::new(prototype_header),
        compile_flags,
        build_mode,
    })?;

    println!(
        "Wrote IntelliSense configuration (compile_commands.json, .clangd, .vscode settings) to {}",
        example_dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;

    fn stub_inputs(root: &Path) -> ClangdConfigInputs {
        let sketch_dir = root.join("Blink");
        let cache = sketch_dir.join(".build").join("wasm");
        fs::create_dir_all(&cache).unwrap();
        let ino_file = sketch_dir.join("Blink.ino");
        fs::write(&ino_file, "void setup() {}\nvoid loop() {}\n").unwrap();
        let prototype_header = sketch_dir
            .join(".fastled")
            .join("intellisense")
            .join("prototypes.hpp");
        fs::create_dir_all(prototype_header.parent().unwrap()).unwrap();
        fs::write(&prototype_header, "#pragma once\n").unwrap();
        let wrapper_source = cache.join("Blink_wrapper.cpp");
        fs::write(&wrapper_source, "// wrapper\n").unwrap();

        let fastled_dir = root.join("fastled");
        fs::create_dir_all(
            fastled_dir
                .join("src")
                .join("platforms")
                .join("wasm")
                .join("compiler"),
        )
        .unwrap();

        let emsdk = root.join("emsdk").join("4.0.19");
        fs::create_dir_all(emsdk.join("bin")).unwrap();
        let clang_name = if cfg!(windows) {
            "clang++.exe"
        } else {
            "clang++"
        };
        fs::write(emsdk.join("bin").join(clang_name), "stub").unwrap();
        fs::create_dir_all(
            emsdk
                .join("emscripten")
                .join("cache")
                .join("sysroot")
                .join("include")
                .join("c++")
                .join("v1"),
        )
        .unwrap();
        fs::create_dir_all(emsdk.join("emscripten").join("system").join("include")).unwrap();

        ClangdConfigInputs {
            sketch_dir: NormalizedPath::new(&sketch_dir),
            fastled_dir: NormalizedPath::new(fastled_dir),
            emsdk_install_dir: NormalizedPath::new(&emsdk),
            tools_emcc_path: NormalizedPath::new(emsdk.join("emscripten").join("emcc.py")),
            tools_empp_path: NormalizedPath::new(emsdk.join("emscripten").join("em++.py")),
            wrapper_source: NormalizedPath::new(wrapper_source),
            ino_files: vec![NormalizedPath::new(ino_file)],
            prototype_header: NormalizedPath::new(prototype_header),
            compile_flags: vec![
                "-DFASTLED_FORCE_NAMESPACE=1".to_string(),
                "-DSKETCH_COMPILE=1".to_string(),
                "-std=c++20".to_string(),
                "-fno-exceptions".to_string(),
                "-sALLOW_MEMORY_GROWTH=1".to_string(),
                "-flto=thin".to_string(),
                "-fmodule-file=/tmp/wasm_pch.h.pcm".to_string(),
                "-Xclang".to_string(),
                "-fmodules-codegen".to_string(),
            ],
            build_mode: BuildMode::Quick,
        }
    }

    #[test]
    fn filter_drops_em_settings_and_module_flags() {
        let flags: Vec<String> = [
            "-std=c++20",
            "-sALLOW_MEMORY_GROWTH=1",
            "-s",
            "EXPORTED_FUNCTIONS=_main",
            "-flto=thin",
            "-flto",
            "-fmodule-file=a.pcm",
            "-Xclang",
            "-fmodules-codegen",
            "-shared",
            "-DSKETCH_COMPILE=1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            filter_compile_flags(&flags),
            vec![
                "-std=c++20".to_string(),
                "-shared".to_string(),
                "-DSKETCH_COMPILE=1".to_string(),
            ]
        );
    }

    #[test]
    fn parses_emscripten_version_from_install_dir() {
        assert_eq!(
            emscripten_version(Path::new(
                "/home/u/.fastled/toolchains/emscripten/x/y/4.0.19"
            )),
            Some((4, 0))
        );
        assert_eq!(emscripten_version(Path::new("/tmp/not-a-version")), None);
    }

    #[test]
    fn writes_all_three_files_with_expected_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let inputs = stub_inputs(tmp.path());

        write_clangd_config(&inputs).unwrap();

        // compile_commands.json
        let cc_path = inputs.sketch_dir.join("compile_commands.json");
        let cc_text = fs::read_to_string(&cc_path).unwrap();
        assert!(!cc_text.contains("\\\\?\\"), "no \\\\?\\ prefixes allowed");
        let cc: Value = serde_json::from_str(&cc_text).unwrap();
        let entries = cc.as_array().expect("array");
        assert_eq!(entries.len(), 2);

        let clangxx = bundled_clangxx(&crate::path::canonicalize_normalized(
            &inputs.emsdk_install_dir,
        ));
        assert!(clangxx.is_file(), "bundled clang++ must exist on disk");
        for entry in entries {
            let args: Vec<&str> = entry["arguments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(args[0], clangxx.to_string_lossy().replace('\\', "/"));
            assert!(args.contains(&"--target=wasm32-unknown-emscripten"));
            assert!(args.contains(&"-D__EMSCRIPTEN__=1"));
            assert!(args.contains(&"-D__EMSCRIPTEN_major__=4"));
            assert!(args
                .iter()
                .any(|a| a.starts_with("--sysroot=") && a.ends_with("emscripten/cache/sysroot")));
            assert!(args.contains(&"-std=c++20"));
            // em-only flags are filtered.
            assert!(!args.contains(&"-sALLOW_MEMORY_GROWTH=1"));
            assert!(!args.contains(&"-flto=thin"));
            assert!(!args.iter().any(|a| a.starts_with("-fmodule-file=")));
            assert!(!args.contains(&"-Xclang"));
            // -I flags use the two-token form: ["-I", "<dir>"].
            let include_dirs: Vec<&str> = args
                .iter()
                .enumerate()
                .filter(|(_, a)| **a == "-I")
                .map(|(i, _)| args[i + 1])
                .collect();
            assert!(include_dirs.iter().any(|d| d.ends_with("fastled/src")));
            assert!(include_dirs
                .iter()
                .any(|d| d.ends_with("src/platforms/wasm/compiler")));
            // directory is the FastLED checkout (build cwd).
            let directory = entry["directory"].as_str().unwrap();
            assert!(directory.ends_with("fastled"));
            assert!(!directory.contains("\\\\?\\"));
        }
        // The .ino entry forces C++.
        let ino_args: Vec<&str> = entries[0]["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(entries[0]["file"].as_str().unwrap().ends_with("Blink.ino"));
        assert_eq!(&ino_args[1..3], ["-x", "c++"]);
        assert!(ino_args.windows(2).any(|pair| pair[0] == "-include"
            && pair[1].ends_with(".fastled/intellisense/prototypes.hpp")));

        // .clangd
        let clangd_text = fs::read_to_string(inputs.sketch_dir.join(".clangd")).unwrap();
        assert!(clangd_text.contains("CompilationDatabase: ."));
        assert!(clangd_text.contains("- --target=wasm32-unknown-emscripten"));
        assert!(clangd_text.contains("- -D__EMSCRIPTEN__=1"));
        assert!(clangd_text.contains("- -DSKETCH_COMPILE=1"));
        assert!(clangd_text.contains("- -fmodule-file=*"));
        assert!(!clangd_text.contains("\\\\?\\"));

        // .vscode/settings.json
        let settings_path = inputs.sketch_dir.join(".vscode").join("settings.json");
        let settings: Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        let clangd_args: Vec<&str> = settings["clangd.arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(clangd_args.contains(&"--compile-commands-dir=${workspaceFolder}"));
        assert!(clangd_args
            .iter()
            .any(|a| a.starts_with("--query-driver=") && a.contains("clang++")));
        assert_eq!(settings["files.associations"]["*.ino"], "cpp");

        let cpptools: Value = serde_json::from_str(
            &fs::read_to_string(
                inputs
                    .sketch_dir
                    .join(".vscode")
                    .join("c_cpp_properties.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            cpptools["configurations"][0]["compileCommands"],
            "${workspaceFolder}/compile_commands.json"
        );
        assert!(cpptools["configurations"][0]["forcedInclude"][0]
            .as_str()
            .unwrap()
            .ends_with(".fastled/intellisense/prototypes.hpp"));
    }

    #[test]
    fn merges_existing_vscode_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let inputs = stub_inputs(tmp.path());
        let settings_path = inputs.sketch_dir.join(".vscode").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(
            &settings_path,
            r#"{"editor.formatOnSave": true, "files.associations": {"*.h": "cpp"}}"#,
        )
        .unwrap();

        write_clangd_config(&inputs).unwrap();

        let settings: Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(settings["editor.formatOnSave"], true);
        assert_eq!(settings["files.associations"]["*.h"], "cpp");
        assert_eq!(settings["files.associations"]["*.ino"], "cpp");
        assert!(settings["clangd.arguments"].is_array());
    }

    #[test]
    fn regenerating_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let inputs = stub_inputs(tmp.path());
        write_clangd_config(&inputs).unwrap();
        let first = fs::read_to_string(inputs.sketch_dir.join("compile_commands.json")).unwrap();
        write_clangd_config(&inputs).unwrap();
        let second = fs::read_to_string(inputs.sketch_dir.join("compile_commands.json")).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn emits_a_forced_prelude_entry_for_every_ino_tab() {
        let tmp = tempfile::tempdir().unwrap();
        let mut inputs = stub_inputs(tmp.path());
        let utility = inputs.sketch_dir.join("Utility.ino");
        fs::write(&utility, "void utility() {}\n").unwrap();
        inputs.ino_files.push(NormalizedPath::new(utility));

        write_clangd_config(&inputs).unwrap();
        let commands: Value = serde_json::from_str(
            &fs::read_to_string(inputs.sketch_dir.join("compile_commands.json")).unwrap(),
        )
        .unwrap();
        let entries = commands.as_array().unwrap();
        assert_eq!(
            entries.len(),
            3,
            "two visible tabs plus generated C++ source"
        );
        for entry in &entries[..2] {
            let arguments = entry["arguments"].as_array().unwrap();
            assert!(arguments.windows(2).any(|pair| {
                pair[0].as_str() == Some("-include")
                    && pair[1]
                        .as_str()
                        .is_some_and(|value| value.ends_with("prototypes.hpp"))
            }));
        }
    }
}
