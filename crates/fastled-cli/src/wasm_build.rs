//! Native WASM build backend.
//!
//! This replaces the Python `internal_wasm_build.py` path for the CLI build
//! flow. Meson/Ninja/emcc are still external tools, but orchestration and
//! Emscripten path resolution are owned by this Rust binary.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{archive, debug_symbols, frontend, install};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Quick,
    Debug,
    Release,
}

impl BuildMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

#[derive(Debug)]
pub struct BuildRequest {
    pub sketch_dir: PathBuf,
    pub build_mode: BuildMode,
    pub profile: bool,
    pub fastled_path: Option<PathBuf>,
    pub force_clean: bool,
}

#[derive(Debug)]
pub struct BuildResult {
    pub success: bool,
    pub output_dir: PathBuf,
    pub duration_secs: f64,
    pub sketch_time_secs: f64,
    pub strategy: String,
    pub output: String,
    /// Resolved FastLED checkout used for this build. Surfaces upward so that
    /// the embedded HTTP server can wire DWARF source resolution against it.
    pub fastled_dir: Option<PathBuf>,
    /// Resolved emscripten install root (the parent of `emscripten/`), if
    /// known. Used to expose `emsdk/` source paths for DWARF entries that
    /// point at emsdk headers.
    pub emsdk_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ToolPaths {
    emscripten_dir: PathBuf,
    emcc: PathBuf,
    empp: PathBuf,
    emar: PathBuf,
    python: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
struct BuildFlagsToml {
    all: Option<FlagSection>,
    sketch: Option<FlagSection>,
    linking: Option<LinkingSection>,
    build_modes: Option<BTreeMap<String, ModeSection>>,
    dwarf: Option<debug_symbols::DwarfPrefixConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct FlagSection {
    defines: Option<Vec<String>>,
    compiler_flags: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct LinkingSection {
    base: Option<FlagList>,
    sketch: Option<FlagList>,
}

#[derive(Debug, Default, Deserialize)]
struct ModeSection {
    flags: Option<Vec<String>>,
    sketch_flags: Option<Vec<String>>,
    link_flags: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct FlagList {
    flags: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct DwarfPathRoots {
    sketch_dir: Option<PathBuf>,
    fastled_dir: PathBuf,
    emsdk_root: Option<PathBuf>,
}

fn resolve_python_executable() -> PathBuf {
    if let Some(path) = std::env::var_os("FASTLED_PYTHON_EXECUTABLE").map(PathBuf::from) {
        if path.is_file() {
            return path;
        }
    }
    if let Some(venv) = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from) {
        let candidate = if cfg!(windows) {
            venv.join("Scripts").join("python.exe")
        } else {
            venv.join("bin").join("python")
        };
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(if cfg!(windows) {
        "python.exe"
    } else {
        "python3"
    })
}

fn resolve_tool_paths(install_dir: &Path) -> Result<ToolPaths> {
    let emscripten_dir = install_dir.join("emscripten");
    let emcc = emscripten_dir.join("emcc.py");
    let empp = emscripten_dir.join("em++.py");
    let emar = emscripten_dir.join("emar.py");
    if !emcc.is_file() {
        bail!("missing emcc.py at {}", emcc.display());
    }
    if !empp.is_file() {
        bail!("missing em++.py at {}", empp.display());
    }
    if !emar.is_file() {
        bail!("missing emar.py at {}", emar.display());
    }
    Ok(ToolPaths {
        emscripten_dir,
        emcc,
        empp,
        emar,
        python: resolve_python_executable(),
    })
}

fn build_env(tools: &ToolPaths) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "EMSCRIPTEN".to_string(),
            tools.emscripten_dir.display().to_string(),
        ),
        (
            "EMSCRIPTEN_ROOT".to_string(),
            tools.emscripten_dir.display().to_string(),
        ),
        (
            "EM_CONFIG".to_string(),
            tools
                .emscripten_dir
                .parent()
                .unwrap_or(&tools.emscripten_dir)
                .join(".emscripten")
                .display()
                .to_string(),
        ),
        (
            "EMSDK_PYTHON".to_string(),
            tools.python.display().to_string(),
        ),
        ("EMCC_SKIP_SANITY_CHECK".to_string(), "1".to_string()),
    ];
    if cfg!(windows) {
        // Match the host's logical core count so emscripten's parallel
        // sub-builds (libc, compiler_rt, libcxx) don't spawn 128 simultaneous
        // emcc subprocesses, which on Windows can crash with access
        // violations under contention. Clamp to a reasonable maximum.
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8)
            .min(32);
        env.push(("EMCC_CORES".to_string(), cores.to_string()));
    }
    env
}

fn command_with_env(program: impl AsRef<Path>, tools: &ToolPaths) -> Command {
    let mut command = Command::new(program.as_ref());
    for (key, value) in build_env(tools) {
        command.env(key, value);
    }
    command
}

fn run_status(mut command: Command, label: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("launch {label}"))?;
    if !status.success() {
        bail!("{label} failed with {status}");
    }
    Ok(())
}

/// Canonicalize and lexically normalize, stripping the Windows `\\?\`
/// long-path prefix that breaks external tools (meson, Python, emcc).
/// See `crate::path::canonicalize_normalized` for details.
fn normalize_path(path: &Path) -> PathBuf {
    crate::path::canonicalize_normalized(path).into_path_buf()
}

fn resolve_example_name(sketch_dir: &Path, fastled_dir: &Path) -> (String, PathBuf, bool) {
    let examples_dir = fastled_dir.join("examples");
    if let Ok(relative) = sketch_dir.strip_prefix(&examples_dir) {
        let name = relative.to_string_lossy().replace('\\', "/");
        return (name, sketch_dir.to_path_buf(), true);
    }
    let name = sketch_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("sketch")
        .to_string();
    (name, sketch_dir.to_path_buf(), false)
}

fn mode_output_dir(sketch_dir: &Path) -> PathBuf {
    sketch_dir.join("fastled_js")
}

fn build_dir(fastled_dir: &Path, mode: BuildMode) -> PathBuf {
    fastled_dir
        .join(".build")
        .join(format!("meson-wasm-{}", mode.as_str()))
}

fn sketch_cache_dir(example_dir: &Path) -> PathBuf {
    example_dir.join(".build").join("wasm")
}

fn sketch_ino_file(example_dir: &Path) -> Result<PathBuf> {
    let sketch_name = example_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("sketch");
    let expected = example_dir.join(format!("{sketch_name}.ino"));
    if expected.is_file() {
        return Ok(expected);
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(example_dir)
        .with_context(|| format!("read sketch directory {}", example_dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("ino") {
            candidates.push(path);
        }
    }
    candidates.sort();
    match candidates.as_slice() {
        [single] => Ok(single.clone()),
        [] => bail!("example has no .ino file: {}", example_dir.display()),
        _ => bail!("example has multiple .ino files: {}", example_dir.display()),
    }
}

fn wrapper_stem(example_name: &str) -> String {
    example_name.replace(['\\', '/'], "_")
}

fn load_build_flags(fastled_dir: &Path) -> Result<BuildFlagsToml> {
    let path = fastled_dir
        .join("src")
        .join("platforms")
        .join("wasm")
        .join("compiler")
        .join("build_flags.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("read build flags {}", path.display()))?;
    toml::from_str(&source).with_context(|| format!("parse {}", path.display()))
}

fn get_sketch_compile_flags(
    fastled_dir: &Path,
    mode: BuildMode,
    dwarf_roots: Option<&DwarfPathRoots>,
) -> Result<Vec<String>> {
    let config = load_build_flags(fastled_dir)?;
    let mode_key = mode.as_str().to_string();
    let mode_config = config
        .build_modes
        .as_ref()
        .and_then(|modes| modes.get(&mode_key));

    let mut flags = Vec::new();
    if let Some(section) = config.all {
        flags.extend(section.defines.unwrap_or_default());
        flags.extend(section.compiler_flags.unwrap_or_default());
    }
    if let Some(section) = config.sketch {
        flags.extend(section.defines.unwrap_or_default());
        flags.extend(section.compiler_flags.unwrap_or_default());
    }
    if let Some(mode_config) = mode_config {
        if let Some(sketch_flags) = &mode_config.sketch_flags {
            flags.extend(sketch_flags.clone());
        } else {
            flags.extend(mode_config.flags.clone().unwrap_or_default());
        }
    }
    if mode == BuildMode::Debug {
        append_dwarf_prefix_maps(&mut flags, config.dwarf.as_ref(), dwarf_roots);
    }
    Ok(flags)
}

fn get_link_flags(fastled_dir: &Path, mode: BuildMode) -> Result<Vec<String>> {
    let config = load_build_flags(fastled_dir)?;
    let mode_key = mode.as_str().to_string();
    let mode_config = config
        .build_modes
        .as_ref()
        .and_then(|modes| modes.get(&mode_key));

    let mut flags = Vec::new();
    if let Some(linking) = config.linking {
        if let Some(base) = linking.base {
            flags.extend(base.flags.unwrap_or_default());
        }
        if let Some(sketch) = linking.sketch {
            flags.extend(sketch.flags.unwrap_or_default());
        }
    }
    if let Some(mode_config) = mode_config {
        flags.extend(mode_config.link_flags.clone().unwrap_or_default());
    }
    if mode == BuildMode::Debug
        && !flags
            .iter()
            .any(|flag| flag == "-gsource-map" || flag.starts_with("-gsource-map="))
    {
        flags.push("-gsource-map".to_string());
    }
    Ok(flags)
}

fn append_dwarf_prefix_maps(
    flags: &mut Vec<String>,
    config: Option<&debug_symbols::DwarfPrefixConfig>,
    roots: Option<&DwarfPathRoots>,
) {
    let prefixes = config.cloned().unwrap_or_default();
    if let Some(roots) = roots {
        if let Some(sketch_dir) = &roots.sketch_dir {
            push_file_prefix_map(flags, sketch_dir, &prefixes.sketch_prefix);
        }
        push_file_prefix_map(
            flags,
            &roots.fastled_dir.join("src"),
            &prefixes.fastled_prefix,
        );
        if let Some(emsdk_root) = &roots.emsdk_root {
            let emsdk_prefix = format!("{}/emsdk", prefixes.dwarf_prefix.trim_matches('/'));
            push_file_prefix_map(flags, emsdk_root, &emsdk_prefix);
        }
        return;
    }

    if let (Some(from), Some(to)) = (
        prefixes.file_prefix_map_from.as_ref(),
        prefixes.file_prefix_map_to.as_ref(),
    ) {
        flags.push(format!(
            "-ffile-prefix-map={}={}",
            normalize_prefix_map_from(from),
            normalize_prefix_map_to(to)
        ));
    }
}

fn push_file_prefix_map(flags: &mut Vec<String>, from: &Path, to: &str) {
    flags.push(format!(
        "-ffile-prefix-map={}={}",
        normalize_prefix_map_path(from),
        normalize_prefix_map_to(to)
    ));
}

fn normalize_prefix_map_path(path: &Path) -> String {
    let mut normalized = normalize_path(path).to_string_lossy().replace('\\', "/");
    if !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

fn normalize_prefix_map_from(path: &str) -> String {
    path.replace('\\', "/")
}

fn normalize_prefix_map_to(prefix: &str) -> String {
    let mut normalized = prefix.trim_matches('/').replace('\\', "/");
    if !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

fn hash_files(root: &Path, files: &[PathBuf]) -> Result<String> {
    let mut hasher = Sha256::new();
    for path in files {
        let full = if path.is_absolute() {
            path.clone()
        } else {
            root.join(path)
        };
        if full.is_file() {
            hasher.update(
                full.strip_prefix(root)
                    .unwrap_or(&full)
                    .to_string_lossy()
                    .as_bytes(),
            );
            hasher.update(fs::metadata(&full)?.len().to_string().as_bytes());
            hasher.update(fs::read(&full)?);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_source_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == ".git" || name == ".build" || name == "build" || name == "fastled_js" {
                continue;
            }
            collect_source_files(root, &path, out)?;
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("cpp" | "c" | "h" | "hpp" | "ipp" | "toml" | "build")
        ) {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
    Ok(())
}

fn compute_source_file_hash(fastled_dir: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_source_files(fastled_dir, &fastled_dir.join("src"), &mut files)?;
    for path in [
        "meson.build",
        "ci/meson/wasm/meson.build",
        "ci/meson/wasm_cross_file.ini",
    ] {
        files.push(PathBuf::from(path));
    }
    files.sort();
    hash_files(fastled_dir, &files)
}

fn write_native_cross_file(fastled_dir: &Path, tools: &ToolPaths) -> Result<PathBuf> {
    let path = fastled_dir
        .join(".build")
        .join("fastled-wasm-cross-file.ini");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Meson's machine-file parser only accepts single-quoted strings — using
    // Rust's {:?} debug format here produces double-quoted strings and meson
    // rejects them on Windows with "Double quotes are not supported".
    let python = meson_quote(&tools.python.to_string_lossy().replace('\\', "/"));
    let emcc = meson_quote(&tools.emcc.to_string_lossy().replace('\\', "/"));
    let empp = meson_quote(&tools.empp.to_string_lossy().replace('\\', "/"));
    let emar = meson_quote(&tools.emar.to_string_lossy().replace('\\', "/"));
    let content = format!(
        r#"[binaries]
c = [{python}, {emcc}]
cpp = [{python}, {empp}]
ar = [{python}, {emar}]
strip = 'true'

[host_machine]
system = 'emscripten'
cpu_family = 'wasm32'
cpu = 'wasm32'
endian = 'little'

[properties]
skip_sanity_check = true
needs_exe_wrapper = true
"#
    );
    fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

/// Wrap a string in single quotes, escaping any embedded apostrophes so the
/// result is a valid meson machine-file string literal.
fn meson_quote(value: &str) -> String {
    let escaped = value.replace('\'', "\\'");
    format!("'{escaped}'")
}

fn ensure_meson_configured(
    fastled_dir: &Path,
    tools: &ToolPaths,
    build_dir: &Path,
    mode: BuildMode,
    force: bool,
) -> Result<()> {
    let marker = build_dir.join(".src_file_list_hash");
    let current_hash = compute_source_file_hash(fastled_dir)?;
    if build_dir.join("build.ninja").is_file() && !force {
        let stored = fs::read_to_string(&marker).unwrap_or_default();
        if stored.trim() == current_hash {
            return Ok(());
        }
    }

    fs::create_dir_all(build_dir)?;
    let cross_file = write_native_cross_file(fastled_dir, tools)?;
    let mut command = Command::new("meson");
    command.current_dir(fastled_dir);
    command.arg("setup");
    if build_dir.join("build.ninja").is_file() {
        command.arg("--reconfigure");
    }
    command
        .arg("--cross-file")
        .arg(cross_file)
        .arg(build_dir)
        .arg(format!("-Dbuild_mode={}", mode.as_str()));
    for (key, value) in build_env(tools) {
        command.env(key, value);
    }
    println!("[WASM] Configuring meson (mode: {})...", mode.as_str());
    run_status(command, "meson setup")?;
    fs::write(marker, current_hash)?;
    Ok(())
}

fn library_archive(build_dir: &Path) -> PathBuf {
    build_dir
        .join("ci")
        .join("meson")
        .join("wasm")
        .join("libfastled.a")
}

fn build_library(fastled_dir: &Path, tools: &ToolPaths, build_dir: &Path) -> Result<bool> {
    let archive = library_archive(build_dir);
    let fingerprint_path = build_dir.join("library_src_fingerprint");
    let current = compute_source_file_hash(fastled_dir)?;
    if archive.is_file()
        && fingerprint_path.is_file()
        && fs::read_to_string(&fingerprint_path)
            .unwrap_or_default()
            .trim()
            == current
    {
        println!("[WASM] Library up-to-date");
        return Ok(false);
    }
    if archive.exists() {
        fs::remove_file(&archive).ok();
    }
    let mut command = Command::new("meson");
    command
        .current_dir(fastled_dir)
        .args(["compile", "-C"])
        .arg(build_dir)
        .arg("fastled");
    for (key, value) in build_env(tools) {
        command.env(key, value);
    }
    println!("[WASM] Building libfastled.a...");
    run_status(command, "meson compile fastled")?;
    fs::write(fingerprint_path, current)?;
    println!("[WASM] Library build successful");
    Ok(true)
}

fn create_wrapper(
    example_dir: &Path,
    example_name: &str,
    sketch_cache_dir: &Path,
) -> Result<PathBuf> {
    let ino_file = sketch_ino_file(example_dir)?;
    fs::create_dir_all(sketch_cache_dir)?;
    let mut lines = vec![
        format!("// Auto-generated wrapper for {example_name}.ino"),
        "// C++20 header unit import; the .ino FastLED include is then a no-op.".to_string(),
        "import \"wasm_pch.h\";".to_string(),
        format!(
            "#include \"{}\"",
            ino_file.to_string_lossy().replace('\\', "/")
        ),
    ];
    let mut extras = Vec::new();
    collect_cpp_files(example_dir, &mut extras)?;
    extras.sort();
    for cpp in extras {
        if cpp != ino_file {
            lines.push(format!(
                "#include \"{}\"",
                cpp.to_string_lossy().replace('\\', "/")
            ));
        }
    }
    let wrapper = sketch_cache_dir.join(format!("{}_wrapper.cpp", wrapper_stem(example_name)));
    let content = lines.join("\n") + "\n";
    if fs::read_to_string(&wrapper).unwrap_or_default() != content {
        fs::write(&wrapper, content)?;
    }
    Ok(wrapper)
}

fn collect_cpp_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name().to_string_lossy() == ".build" {
                continue;
            }
            collect_cpp_files(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("cpp") {
            out.push(path);
        }
    }
    Ok(())
}

fn build_sketch_pch(
    fastled_dir: &Path,
    tools: &ToolPaths,
    build_dir: &Path,
    mode: BuildMode,
    emsdk_root: Option<&Path>,
    lib_was_rebuilt: bool,
) -> Result<Option<PathBuf>> {
    let pcm = build_dir.join("wasm_pch.h.pcm");
    let pch_o = build_dir.join("pch_codegen.o");
    let hash_path = build_dir.join("sketch_pch.hash");
    let header = fastled_dir
        .join("src")
        .join("platforms")
        .join("wasm")
        .join("compiler")
        .join("wasm_pch.h");
    if !header.is_file() {
        return Ok(None);
    }
    let dwarf_roots = DwarfPathRoots {
        sketch_dir: None,
        fastled_dir: fastled_dir.to_path_buf(),
        emsdk_root: emsdk_root.map(Path::to_path_buf),
    };
    let compile_flags = get_sketch_compile_flags(fastled_dir, mode, Some(&dwarf_roots))?;
    let mut hash_input = compile_flags.join("\n");
    hash_input.push_str(&compute_source_file_hash(fastled_dir)?);
    let current_hash = format!("{:x}", Sha256::digest(hash_input.as_bytes()));
    if !lib_was_rebuilt
        && pcm.is_file()
        && pch_o.is_file()
        && fs::read_to_string(&hash_path).unwrap_or_default().trim() == current_hash
    {
        return Ok(Some(pcm));
    }

    let mut args = vec![
        "-fmodule-header=user".to_string(),
        header.display().to_string(),
        "-o".to_string(),
        pcm.display().to_string(),
        "-D_LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_FAST".to_string(),
        "-D_FILE_OFFSET_BITS=64".to_string(),
    ];
    args.extend(compile_flags);
    args.push(format!("-I{}", fastled_dir.join("src").display()));
    args.push(format!(
        "-I{}",
        fastled_dir
            .join("src")
            .join("platforms")
            .join("wasm")
            .join("compiler")
            .display()
    ));
    args.extend(["-Xclang".to_string(), "-fmodules-codegen".to_string()]);

    println!("[WASM] Building sketch header unit (.pcm)...");
    let mut command = command_with_env(&tools.python, tools);
    command
        .current_dir(fastled_dir)
        .arg(&tools.emcc)
        .args(&args);
    run_status(command, "emcc header unit")?;

    let mut command = command_with_env(&tools.python, tools);
    command
        .current_dir(fastled_dir)
        .arg(&tools.emcc)
        .args(["-c"])
        .arg(&pcm)
        .args(["-o"])
        .arg(&pch_o)
        .args(["-O0", "-g0"]);
    if run_status(command, "emcc header unit companion").is_ok() {
        let archive = library_archive(build_dir);
        if archive.is_file() && tools.emar.is_file() {
            let mut command = command_with_env(&tools.python, tools);
            command
                .current_dir(fastled_dir)
                .arg(&tools.emar)
                .arg("r")
                .arg(&archive)
                .arg(&pch_o);
            let _ = run_status(command, "emar pch companion");
        }
    }

    fs::write(hash_path, current_hash)?;
    Ok(Some(pcm))
}

fn compile_sketch(
    tools: &ToolPaths,
    wrapper: &Path,
    build_dir: &Path,
    sketch_cache_dir: &Path,
    example_dir: &Path,
    mode: BuildMode,
    dwarf_roots: &DwarfPathRoots,
) -> Result<PathBuf> {
    let fastled_dir = &dwarf_roots.fastled_dir;
    let object = sketch_cache_dir.join("sketch.o");
    let archive = library_archive(build_dir);
    let ino = sketch_ino_file(example_dir)?;
    let compile_flags = get_sketch_compile_flags(fastled_dir, mode, Some(dwarf_roots))?;
    let flags_hash_path = sketch_cache_dir.join("sketch_compile_flags.hash");
    let current_flags_hash = format!("{:x}", Sha256::digest(compile_flags.join("\n").as_bytes()));
    if object.is_file() {
        let object_mtime = object.metadata()?.modified()?;
        let inputs = [wrapper, &ino, &archive];
        if fs::read_to_string(&flags_hash_path)
            .unwrap_or_default()
            .trim()
            == current_flags_hash
            && inputs
                .iter()
                .filter_map(|path| path.metadata().ok())
                .filter_map(|metadata| metadata.modified().ok())
                .all(|mtime| mtime <= object_mtime)
        {
            println!("[WASM] Sketch is up-to-date");
            return Ok(object);
        }
    }

    let mut args = vec![
        "-c".to_string(),
        wrapper.display().to_string(),
        "-o".to_string(),
        object.display().to_string(),
    ];
    args.extend(compile_flags);
    args.push(format!("-I{}", fastled_dir.join("src").display()));
    args.push(format!(
        "-I{}",
        fastled_dir
            .join("src")
            .join("platforms")
            .join("wasm")
            .join("compiler")
            .display()
    ));
    args.push(format!("-I{}", example_dir.display()));
    let pcm = build_dir.join("wasm_pch.h.pcm");
    if pcm.is_file() {
        args.push(format!("-fmodule-file={}", pcm.display()));
    }

    println!("[WASM] Compiling sketch: {}", wrapper.display());
    let mut command = command_with_env(&tools.python, tools);
    command
        .current_dir(fastled_dir)
        .arg(&tools.empp)
        .args(&args);
    run_status(command, "em++ sketch compile")?;
    fs::write(flags_hash_path, current_flags_hash)?;
    Ok(object)
}

fn link_wasm(
    fastled_dir: &Path,
    tools: &ToolPaths,
    sketch_object: &Path,
    build_dir: &Path,
    sketch_cache_dir: &Path,
    output_js: &Path,
    mode: BuildMode,
) -> Result<()> {
    let archive = library_archive(build_dir);
    let cached_js = sketch_cache_dir.join("fastled.js");
    let cached_wasm = sketch_cache_dir.join("fastled.wasm");
    let link_flags = get_link_flags(fastled_dir, mode)?;
    let flags_hash_path = sketch_cache_dir.join("link_flags.hash");
    let current_flags_hash = format!("{:x}", Sha256::digest(link_flags.join("\n").as_bytes()));
    if cached_js.is_file() && cached_wasm.is_file() {
        let output_mtime = cached_js.metadata()?.modified()?;
        let inputs = [sketch_object, &archive];
        if fs::read_to_string(&flags_hash_path)
            .unwrap_or_default()
            .trim()
            == current_flags_hash
            && inputs
                .iter()
                .filter_map(|path| path.metadata().ok())
                .filter_map(|metadata| metadata.modified().ok())
                .all(|mtime| mtime <= output_mtime)
        {
            copy_linked_output(sketch_cache_dir, output_js)?;
            println!("[WASM] Link output up-to-date");
            return Ok(());
        }
    }

    let js_library = fastled_dir
        .join("src")
        .join("platforms")
        .join("wasm")
        .join("compiler")
        .join("js_library.js");
    let mut args = vec![
        sketch_object.display().to_string(),
        archive.display().to_string(),
        format!("-I{}", fastled_dir.join("src").display()),
        format!(
            "-I{}",
            fastled_dir
                .join("src")
                .join("platforms")
                .join("wasm")
                .display()
        ),
        format!(
            "-I{}",
            fastled_dir
                .join("src")
                .join("platforms")
                .join("wasm")
                .join("compiler")
                .display()
        ),
        format!("--js-library={}", js_library.display()),
        "-o".to_string(),
        cached_js.display().to_string(),
    ];
    args.extend(link_flags);

    println!("[WASM] Linking final WASM module...");
    run_em_link_with_retries(fastled_dir, tools, &args)?;
    fs::write(flags_hash_path, current_flags_hash)?;
    copy_linked_output(sketch_cache_dir, output_js)?;
    Ok(())
}

/// On Windows, emscripten builds its sysroot libraries (libc, compiler_rt,
/// libc++…) from source the first time we link. Individual `emcc`
/// subprocesses occasionally crash mid-build with a non-deterministic
/// access-violation-style exit code; the surrounding em++ link then aborts.
/// Because each completed library is cached to disk, retrying advances the
/// build by one library each time. A small retry loop lets the link
/// converge without exposing the upstream flakiness to users. See #116.
fn run_em_link_with_retries(fastled_dir: &Path, tools: &ToolPaths, args: &[String]) -> Result<()> {
    let max_attempts = if cfg!(windows) { 6 } else { 1 };
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=max_attempts {
        let mut command = command_with_env(&tools.python, tools);
        command.current_dir(fastled_dir).arg(&tools.empp).args(args);
        match run_status(command, "em++ wasm link") {
            Ok(()) => return Ok(()),
            Err(err) => {
                if attempt < max_attempts {
                    println!(
                        "[WASM] em++ link attempt {attempt}/{max_attempts} failed; \
                         retrying (sysroot libs cache progressively): {err:#}"
                    );
                }
                last_err = Some(err);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("em++ wasm link failed")))
}

fn copy_linked_output(sketch_cache_dir: &Path, output_js: &Path) -> Result<()> {
    let output_dir = output_js
        .parent()
        .ok_or_else(|| anyhow::anyhow!("output path has no parent: {}", output_js.display()))?;
    fs::create_dir_all(output_dir)?;
    for name in [
        "fastled.js",
        "fastled.wasm",
        "fastled.wasm.map",
        "fastled.js.map",
    ] {
        let src = sketch_cache_dir.join(name);
        let dst = output_dir.join(name);
        if src.is_file() {
            fs::copy(&src, &dst)
                .with_context(|| format!("copy {} to {}", src.display(), output_dir.display()))?;
        } else if dst.exists() {
            fs::remove_file(&dst).with_context(|| format!("remove stale {}", dst.display()))?;
        }
    }
    Ok(())
}

fn generate_manifest(example_dir: &Path, output_dir: &Path) -> Result<()> {
    let mut files = Vec::<serde_json::Value>::new();
    collect_data_files(example_dir, example_dir, &mut files)?;
    fs::write(
        output_dir.join("files.json"),
        serde_json::to_string_pretty(&files)?,
    )?;
    Ok(())
}

fn collect_data_files(root: &Path, dir: &Path, out: &mut Vec<serde_json::Value>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "fastled_js" || name == ".build" {
                continue;
            }
            collect_data_files(root, &path, out)?;
        } else if matches!(
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase())
                .as_deref(),
            Some("json" | "csv" | "txt" | "cfg" | "bin" | "dat" | "mp3" | "wav")
        ) {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
            out.push(serde_json::json!({
                "path": rel.replace('\\', "/"),
                "size": path.metadata()?.len(),
            }));
        }
    }
    Ok(())
}

fn resolve_fastled_dir(request: &BuildRequest) -> Result<(PathBuf, Option<PathBuf>)> {
    if let Some(path) = &request.fastled_path {
        return Ok((normalize_path(path), None));
    }
    // Honour the ref written into `<sketch>/fastled.json` by `fastled --init`
    // so the build always uses the same FastLED checkout the sketch was
    // scaffolded against (master example layout drifts from older releases).
    // Falls back to `master` only when no pin is recorded.
    let pinned_ref = crate::project::read_fastled_json_ref(&request.sketch_dir);
    if let Some(local_path) = pinned_ref
        .as_deref()
        .filter(|s| Path::new(s).join("library.json").is_file())
    {
        return Ok((normalize_path(Path::new(local_path)), None));
    }
    let ref_arg = pinned_ref.as_deref().or(Some("master"));
    let repo = install::ensure_fastled_repo(ref_arg)?;
    let cache_root = repo
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repo.clone());
    let short_dir = cache_root.join("fl").join("repo");
    if short_dir == repo {
        return Ok((repo, None));
    }
    let marker = short_dir.join(".fastled-source");
    let source_key = repo.to_string_lossy().into_owned();
    if short_dir.join("library.json").is_file()
        && fs::read_to_string(&marker).unwrap_or_default() == source_key
    {
        return Ok((short_dir, None));
    }
    if short_dir.exists() {
        fs::remove_dir_all(&short_dir).ok();
    }
    if let Some(parent) = short_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    copy_dir(&repo, &short_dir)?;
    fs::write(marker, source_key)?;
    Ok((short_dir, None))
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            if entry.file_name().to_string_lossy() == ".git" {
                continue;
            }
            copy_dir(&path, &target)?;
        } else {
            fs::copy(&path, &target)
                .with_context(|| format!("copy {} to {}", path.display(), target.display()))?;
        }
    }
    Ok(())
}

pub fn run_build(request: &BuildRequest) -> Result<BuildResult> {
    let wall_start = std::time::Instant::now();
    let output_dir = mode_output_dir(&request.sketch_dir);
    if request.force_clean && output_dir.exists() {
        fs::remove_dir_all(&output_dir).ok();
    }
    fs::create_dir_all(&output_dir)?;

    let emscripten_install = install::ensure_emscripten_installed()?;
    archive::write_emscripten_config(&emscripten_install, "node")?;
    let tools = resolve_tool_paths(&emscripten_install)?;
    let (fastled_dir, _cleanup) = resolve_fastled_dir(request)?;
    let fastled_dir = normalize_path(&fastled_dir);
    let emsdk_root = emscripten_install
        .parent()
        .map(Path::to_path_buf)
        .or_else(|| Some(emscripten_install.clone()));
    let (example_name, example_dir, _is_in_tree) =
        resolve_example_name(&normalize_path(&request.sketch_dir), &fastled_dir);

    let sketch_start = std::time::Instant::now();
    let strategy = if output_dir.join("fastled.js").is_file()
        && output_dir.join("fastled.wasm").is_file()
        && !request.force_clean
    {
        "incremental"
    } else {
        "cold"
    }
    .to_string();

    let build_result = (|| -> Result<()> {
        let build_dir = build_dir(&fastled_dir, request.build_mode);
        ensure_meson_configured(
            &fastled_dir,
            &tools,
            &build_dir,
            request.build_mode,
            request.force_clean,
        )?;
        let lib_rebuilt = build_library(&fastled_dir, &tools, &build_dir)?;
        let _ = build_sketch_pch(
            &fastled_dir,
            &tools,
            &build_dir,
            request.build_mode,
            emsdk_root.as_deref(),
            lib_rebuilt,
        );
        let sketch_cache = sketch_cache_dir(&example_dir);
        let wrapper = create_wrapper(&example_dir, &example_name, &sketch_cache)?;
        let sketch_dwarf_roots = DwarfPathRoots {
            sketch_dir: Some(example_dir.clone()),
            fastled_dir: fastled_dir.clone(),
            emsdk_root: emsdk_root.clone(),
        };
        let sketch_o = compile_sketch(
            &tools,
            &wrapper,
            &build_dir,
            &sketch_cache,
            &example_dir,
            request.build_mode,
            &sketch_dwarf_roots,
        )?;
        let output_js = output_dir.join("fastled.js");
        link_wasm(
            &fastled_dir,
            &tools,
            &sketch_o,
            &build_dir,
            &sketch_cache,
            &output_js,
            request.build_mode,
        )?;
        frontend::copy_frontend_to_output(&output_dir, None)?;
        generate_manifest(&example_dir, &output_dir)?;
        let debug_config = debug_symbols::load_debug_symbol_config(
            example_dir.clone(),
            Some(fastled_dir.clone()),
            emsdk_root.clone(),
        );
        debug_symbols::write_debug_symbol_manifest(&output_dir, &debug_config)?;
        Ok(())
    })();

    match build_result {
        Ok(()) => Ok(BuildResult {
            success: true,
            output_dir,
            duration_secs: wall_start.elapsed().as_secs_f64(),
            sketch_time_secs: sketch_start.elapsed().as_secs_f64(),
            strategy,
            output: "Native Rust WASM build successful".to_string(),
            fastled_dir: Some(fastled_dir.clone()),
            emsdk_root: emsdk_root.clone(),
        }),
        Err(err) => Ok(BuildResult {
            success: false,
            output_dir,
            duration_secs: wall_start.elapsed().as_secs_f64(),
            sketch_time_secs: sketch_start.elapsed().as_secs_f64(),
            strategy,
            output: format!("Native Rust WASM build failed: {err:#}"),
            fastled_dir: Some(fastled_dir.clone()),
            emsdk_root: emsdk_root.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_build_flags(fastled_dir: &Path, source: &str) {
        let compiler_dir = fastled_dir.join("src/platforms/wasm/compiler");
        fs::create_dir_all(&compiler_dir).unwrap();
        fs::write(compiler_dir.join("build_flags.toml"), source).unwrap();
    }

    #[test]
    fn resolve_example_preserves_nested_names() {
        let root = PathBuf::from("/tmp/FastLED");
        let sketch = root.join("examples").join("Fx").join("FxCylon");
        let (name, dir, in_tree) = resolve_example_name(&sketch, &root);
        assert_eq!(name, "Fx/FxCylon");
        assert_eq!(dir, sketch);
        assert!(in_tree);
    }

    #[test]
    fn meson_quote_wraps_in_single_quotes() {
        assert_eq!(meson_quote("simple"), "'simple'");
        assert_eq!(
            meson_quote("C:/path/with spaces/em.py"),
            "'C:/path/with spaces/em.py'"
        );
    }

    #[test]
    fn meson_quote_escapes_embedded_apostrophes() {
        assert_eq!(meson_quote("it's"), r"'it\'s'");
    }

    #[test]
    fn resolve_example_maps_external_to_examples_leaf() {
        let root = PathBuf::from("/tmp/FastLED");
        let sketch = PathBuf::from("/tmp/MySketch");
        let (name, dir, in_tree) = resolve_example_name(&sketch, &root);
        assert_eq!(name, "MySketch");
        assert_eq!(dir, sketch);
        assert!(!in_tree);
    }

    #[test]
    fn build_mode_strings_match_meson_modes() {
        assert_eq!(BuildMode::Quick.as_str(), "quick");
        assert_eq!(BuildMode::Debug.as_str(), "debug");
        assert_eq!(BuildMode::Release.as_str(), "release");
    }

    #[test]
    fn debug_compile_flags_include_dynamic_dwarf_prefix_maps() {
        let tmp = tempfile::tempdir().unwrap();
        let fastled_dir = tmp.path().join("FastLED");
        let sketch_dir = tmp.path().join("Blink");
        let emsdk_root = tmp.path().join("emsdk");
        write_build_flags(
            &fastled_dir,
            r#"
[all]
defines = []
compiler_flags = []

[sketch]
defines = []
compiler_flags = []

[build_modes.debug]
flags = ["-g3"]

[dwarf]
fastled_prefix = "fastledsource"
sketch_prefix = "sketchsource"
dwarf_prefix = "dwarfsource"
"#,
        );
        let roots = DwarfPathRoots {
            sketch_dir: Some(sketch_dir.clone()),
            fastled_dir: fastled_dir.clone(),
            emsdk_root: Some(emsdk_root.clone()),
        };

        let flags = get_sketch_compile_flags(&fastled_dir, BuildMode::Debug, Some(&roots)).unwrap();

        assert!(flags.contains(&format!(
            "-ffile-prefix-map={}={}",
            normalize_prefix_map_path(&sketch_dir),
            "sketchsource/"
        )));
        assert!(flags.contains(&format!(
            "-ffile-prefix-map={}={}",
            normalize_prefix_map_path(&fastled_dir.join("src")),
            "fastledsource/"
        )));
        assert!(flags.contains(&format!(
            "-ffile-prefix-map={}={}",
            normalize_prefix_map_path(&emsdk_root),
            "dwarfsource/emsdk/"
        )));
    }

    #[test]
    fn quick_compile_flags_do_not_include_dwarf_prefix_maps() {
        let tmp = tempfile::tempdir().unwrap();
        let fastled_dir = tmp.path().join("FastLED");
        write_build_flags(
            &fastled_dir,
            r#"
[all]
defines = []
compiler_flags = []

[sketch]
defines = []
compiler_flags = []

[build_modes.quick]
flags = ["-g0"]
"#,
        );
        let roots = DwarfPathRoots {
            sketch_dir: Some(tmp.path().join("Blink")),
            fastled_dir: fastled_dir.clone(),
            emsdk_root: None,
        };

        let flags = get_sketch_compile_flags(&fastled_dir, BuildMode::Quick, Some(&roots)).unwrap();

        assert!(!flags
            .iter()
            .any(|flag| flag.starts_with("-ffile-prefix-map=")));
    }

    #[test]
    fn debug_link_flags_emit_wasm_source_map() {
        let tmp = tempfile::tempdir().unwrap();
        let fastled_dir = tmp.path().join("FastLED");
        write_build_flags(
            &fastled_dir,
            r#"
[linking.base]
flags = ["-sWASM=1"]

[linking.sketch]
flags = []

[build_modes.debug]
link_flags = []
"#,
        );

        let flags = get_link_flags(&fastled_dir, BuildMode::Debug).unwrap();

        assert!(flags.contains(&"-gsource-map".to_string()));
    }

    #[test]
    fn copy_linked_output_copies_and_removes_source_maps() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let output = tmp.path().join("out").join("fastled.js");
        fs::create_dir_all(&cache).unwrap();
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(cache.join("fastled.js"), "js").unwrap();
        fs::write(cache.join("fastled.wasm"), "wasm").unwrap();
        fs::write(cache.join("fastled.wasm.map"), "map").unwrap();

        copy_linked_output(&cache, &output).unwrap();
        assert_eq!(
            fs::read_to_string(output.parent().unwrap().join("fastled.wasm.map")).unwrap(),
            "map"
        );

        fs::remove_file(cache.join("fastled.wasm.map")).unwrap();
        copy_linked_output(&cache, &output).unwrap();
        assert!(!output.parent().unwrap().join("fastled.wasm.map").exists());
    }
}
