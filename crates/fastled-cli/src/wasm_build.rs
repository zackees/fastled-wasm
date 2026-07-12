//! Native WASM build backend.
//!
//! This replaces the Python `internal_wasm_build.py` path for the CLI build
//! flow. Meson/Ninja/emcc are still external tools, but orchestration and
//! Emscripten path resolution are owned by this Rust binary.

use std::collections::BTreeMap;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cli::LinkMode;
use crate::{archive, debug_symbols, dynamic_cache, frontend, install};

/// Receives one build-output line at a time. The second argument is the
/// originating stream: `"stdout"` or `"stderr"`. Called on the build thread.
pub type LogSink<'a> = &'a (dyn Fn(&str, &str) + 'a);

fn stdio_log(line: &str, stream: &str) {
    if stream == "stderr" {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}

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
    /// Emit clangd/VS Code configuration into the sketch directory after a
    /// successful build. Off by default; opted in via `fastled --clangd`.
    pub emit_clangd: bool,
    pub no_app: bool,
    pub(crate) link_mode: LinkMode,
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
    clangpp: PathBuf,
    wasm_ld: PathBuf,
    llvm_objcopy: PathBuf,
    wasm_finalize: PathBuf,
    emscripten_version: String,
    python: PathBuf,
}

#[derive(Debug, Clone)]
struct BuildFingerprints {
    fastled_source: String,
    toolchain: String,
    library: String,
    source_duration_secs: f64,
    toolchain_duration_secs: f64,
}

#[derive(Debug)]
struct CompiledSketch {
    object: crate::path::NormalizedPath,
    fingerprint: String,
}

impl BuildFingerprints {
    fn compute(
        fastled_dir: &Path,
        tools: &ToolPaths,
        mode: BuildMode,
        link_mode: LinkMode,
    ) -> Result<Self> {
        let ((fastled_source, source_duration_secs), (toolchain, toolchain_duration_secs)) =
            std::thread::scope(|scope| {
                let source = scope.spawn(|| {
                    let started = std::time::Instant::now();
                    compute_source_file_hash(fastled_dir)
                        .map(|value| (value, started.elapsed().as_secs_f64()))
                });
                let toolchain = scope.spawn(|| {
                    let started = std::time::Instant::now();
                    toolchain_fingerprint(tools)
                        .map(|value| (value, started.elapsed().as_secs_f64()))
                });
                let source = source
                    .join()
                    .map_err(|_| anyhow::anyhow!("FastLED fingerprint worker panicked"))??;
                let toolchain = toolchain
                    .join()
                    .map_err(|_| anyhow::anyhow!("toolchain fingerprint worker panicked"))??;
                Ok::<_, anyhow::Error>((source, toolchain))
            })?;
        let mode_value = format!("mode={};link-mode={link_mode:?}", mode.as_str());
        let library = dynamic_cache::fingerprint_values([
            fastled_source.as_bytes(),
            toolchain.as_bytes(),
            mode_value.as_bytes(),
        ]);
        Ok(Self {
            fastled_source,
            toolchain,
            library,
            source_duration_secs,
            toolchain_duration_secs,
        })
    }
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
pub(crate) struct DwarfPathRoots {
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
    let clangpp = install_dir.join("bin").join(if cfg!(windows) {
        "clang++.exe"
    } else {
        "clang++"
    });
    let tool_name = |name: &str| {
        install_dir.join("bin").join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        })
    };
    let wasm_ld = tool_name("wasm-ld");
    let llvm_objcopy = tool_name("llvm-objcopy");
    let wasm_finalize = tool_name("wasm-emscripten-finalize");
    let emscripten_version = fs::read_to_string(emscripten_dir.join("emscripten-version.txt"))
        .ok()
        .and_then(|value| serde_json::from_str::<String>(value.trim()).ok())
        .unwrap_or_default();
    if !emcc.is_file() {
        bail!("missing emcc.py at {}", emcc.display());
    }
    if !empp.is_file() {
        bail!("missing em++.py at {}", empp.display());
    }
    if !emar.is_file() {
        bail!("missing emar.py at {}", emar.display());
    }
    if !clangpp.is_file() {
        bail!("missing clang++ at {}", clangpp.display());
    }
    Ok(ToolPaths {
        emscripten_dir,
        emcc,
        empp,
        emar,
        clangpp,
        wasm_ld,
        llvm_objcopy,
        wasm_finalize,
        emscripten_version,
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
        // Build tools are Python-driven; without this their stdout is
        // block-buffered when piped, which would defeat live log streaming.
        ("PYTHONUNBUFFERED".to_string(), "1".to_string()),
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

const DIRECT_CFLAGS_SCHEMA: u32 = 1;
const DIRECT_CFLAGS_FILE: &str = ".fastled-direct-cflags.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct DirectCflagsCache {
    schema: u32,
    entries: BTreeMap<String, Vec<String>>,
}

fn direct_cflags_key(toolchain_fingerprint: &str, driver_args: &[String]) -> String {
    let mut values = vec![toolchain_fingerprint.to_string()];
    values.extend(driver_args.iter().cloned());
    let mut environment = std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.to_string_lossy();
            (key.starts_with("EMCC_") || matches!(key.as_ref(), "CFLAGS" | "CXXFLAGS"))
                .then(|| format!("env:{key}={}", value.to_string_lossy()))
        })
        .collect::<Vec<_>>();
    environment.sort();
    values.extend(environment);
    dynamic_cache::fingerprint_values(values.iter().map(|value| value.as_bytes()))
}

/// Ask the active Emscripten driver for the backend flags it would pass to
/// clang, then persist them by toolchain + driver arguments. This keeps the
/// supported `em++ --cflags` contract while removing Python startup from warm
/// sketch compiles, including new CLI processes.
fn direct_clang_cflags(
    tools: &ToolPaths,
    toolchain_fingerprint: &str,
    driver_args: &[String],
    current_dir: &Path,
) -> Result<Vec<String>> {
    let install_dir = tools
        .emscripten_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Emscripten directory has no install parent"))?;
    let cache_path = install_dir.join(DIRECT_CFLAGS_FILE);
    let lock_path = install_dir.join(format!("{DIRECT_CFLAGS_FILE}.lock"));
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("open direct cflags lock {}", lock_path.display()))?;
    fs2::FileExt::lock_exclusive(&lock)
        .with_context(|| format!("lock direct cflags cache {}", cache_path.display()))?;

    let key = direct_cflags_key(toolchain_fingerprint, driver_args);
    let mut cache = fs::read(&cache_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<DirectCflagsCache>(&bytes).ok())
        .filter(|cache| cache.schema == DIRECT_CFLAGS_SCHEMA)
        .unwrap_or_else(|| DirectCflagsCache {
            schema: DIRECT_CFLAGS_SCHEMA,
            entries: BTreeMap::new(),
        });
    if let Some(flags) = cache.entries.get(&key) {
        return Ok(flags.clone());
    }

    let mut command = command_with_env(&tools.python, tools);
    command
        .current_dir(current_dir)
        .arg(&tools.empp)
        .args(driver_args)
        .arg("--cflags");
    let output = command.output().context("run em++ --cflags")?;
    if !output.status.success() {
        bail!(
            "em++ --cflags failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8(output.stdout).context("em++ --cflags returned non-UTF-8")?;
    let flags = shell_words::split(stdout.trim()).context("parse em++ --cflags output")?;
    if flags.is_empty() {
        bail!("em++ --cflags returned no backend flags");
    }
    cache.entries.insert(key, flags.clone());
    fs::write(&cache_path, serde_json::to_vec_pretty(&cache)?)
        .with_context(|| format!("write direct cflags cache {}", cache_path.display()))?;
    Ok(flags)
}

fn spawn_line_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    stream: &'static str,
    tx: std::sync::mpsc::Sender<(String, &'static str)>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    while matches!(buf.last(), Some(b'\n' | b'\r')) {
                        buf.pop();
                    }
                    if tx
                        .send((String::from_utf8_lossy(&buf).into_owned(), stream))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}

/// Run a build tool with piped stdout/stderr, forwarding each output line to
/// `log` as it arrives so callers can mirror it to the terminal and the
/// browser SSE stream in real time (#153).
fn run_status(mut command: Command, label: &str, log: LogSink) -> Result<()> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().with_context(|| format!("launch {label}"))?;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(spawn_line_reader(stdout, "stdout", tx.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(spawn_line_reader(stderr, "stderr", tx.clone()));
    }
    drop(tx);
    for (line, stream) in rx {
        log(&line, stream);
    }
    for reader in readers {
        let _ = reader.join();
    }
    let status = child.wait().with_context(|| format!("wait for {label}"))?;
    if !status.success() {
        bail!("{label} failed with {status}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn link_wasm_dynamic(
    fastled_dir: &Path,
    tools: &ToolPaths,
    sketch: &CompiledSketch,
    build_dir: &Path,
    sketch_cache_dir: &Path,
    output_js: &Path,
    mode: BuildMode,
    fingerprints: &BuildFingerprints,
    log: LogSink,
) -> Result<()> {
    let sketch_object = &sketch.object;
    let archive = library_archive(build_dir);
    let link_flags = get_link_flags(fastled_dir, mode)?;
    let main_link_flags = link_flags
        .iter()
        .filter(|flag| flag.as_str() != "-Wl,--no-export-dynamic")
        .cloned()
        .collect::<Vec<_>>();
    // Dynamic linking requires an identical ABI for the main and side
    // modules. Emscripten's default WASM_BIGINT behavior differs between
    // ordinary and SIDE_MODULE links, so make the setting explicit on both
    // sides. This prevents i64 imports from failing instantiation with an
    // "imported function does not match the expected type" LinkError.
    let wasm_bigint_flag = dynamic_wasm_bigint_flag(&link_flags);
    let side_module_link_flags = vec![wasm_bigint_flag.clone()];
    let source_fingerprint = &fingerprints.fastled_source;
    let toolchain_fingerprint = &fingerprints.toolchain;
    let generated_cross_file = fastled_dir
        .join(".build")
        .join("fastled-wasm-cross-file.ini");
    let generated_cross_file_contents = fs::read(&generated_cross_file).unwrap_or_default();
    let mut runtime_values = vec![
        "schema=1".to_string(),
        format!("cli={}", env!("CARGO_PKG_VERSION")),
        format!("mode={}", mode.as_str()),
        "link-mode=dynamic".to_string(),
        "main-module=1".to_string(),
        "autoload-dylibs=0".to_string(),
        "error-on-undefined-symbols=0".to_string(),
        "export-dynamic=1".to_string(),
        "wasm-ld-export-all=1".to_string(),
        "export-all=1".to_string(),
        "whole-archive=1".to_string(),
        format!("source={source_fingerprint}"),
        format!("toolchain={toolchain_fingerprint}"),
        format!("wasm-bigint={wasm_bigint_flag}"),
        format!(
            "loader={}",
            dynamic_cache::fingerprint_values([dynamic_cache::DYNAMIC_LOADER_JS.as_bytes()])
        ),
        format!(
            "cross-file={}",
            dynamic_cache::fingerprint_values([generated_cross_file_contents.as_slice()])
        ),
    ];
    runtime_values.extend(
        main_link_flags
            .iter()
            .map(|flag| format!("link-flag={flag}")),
    );
    runtime_values.extend(
        build_env(tools)
            .into_iter()
            .map(|(key, value)| format!("env:{key}={value}")),
    );
    let runtime_fingerprint =
        dynamic_cache::fingerprint_values(runtime_values.iter().map(|value| value.as_bytes()));
    let runtime_root = build_dir.join("dynamic-runtime-cache");
    let runtime_entry = dynamic_cache::entry_path(&runtime_root, &runtime_fingerprint);
    let runtime_phase_start = std::time::Instant::now();
    let _runtime_lock = dynamic_cache::CacheLock::acquire(&runtime_root, &runtime_fingerprint)?;
    match dynamic_cache::validate_entry(
        &runtime_entry,
        &runtime_fingerprint,
        &["fastled.js", "fastled.wasm"],
    ) {
        Ok(()) => {
            dynamic_cache::clear_attempt(&runtime_root, &runtime_fingerprint)?;
            log(
                &format!(
                    "[WASM] Dynamic runtime cache hit: {} ({:.2}s)",
                    &runtime_fingerprint[..12],
                    runtime_phase_start.elapsed().as_secs_f64()
                ),
                "stdout",
            );
        }
        Err(reason) => {
            let prior = dynamic_cache::previous_attempt(&runtime_root, &runtime_fingerprint)
                .map(|attempt| format!("; previous attempt: {attempt}"))
                .unwrap_or_default();
            log(
                &format!(
                    "[WASM] Dynamic runtime cache miss: {} ({reason}{prior})",
                    &runtime_fingerprint[..12],
                ),
                "stdout",
            );
            dynamic_cache::mark_pending(&runtime_root, &runtime_fingerprint, "main-link")?;
            let rebuild = (|| -> Result<()> {
                let staging = dynamic_cache::staging_dir(&runtime_root, ".runtime-staging-")?;
                let loader = staging.path().join("dynamic_loader.js");
                fs::write(&loader, dynamic_cache::DYNAMIC_LOADER_JS)?;
                let js_library = fastled_dir
                    .join("src")
                    .join("platforms")
                    .join("wasm")
                    .join("compiler")
                    .join("js_library.js");
                let cached_js = staging.path().join("fastled.js");
                let mut main_args = vec![
                    "-Wl,--whole-archive".to_string(),
                    archive.display().to_string(),
                    "-Wl,--no-whole-archive".to_string(),
                    format!("-I{}", fastled_dir.join("src").display()),
                    format!("-I{}", fastled_dir.join("src/platforms/wasm").display()),
                    format!(
                        "-I{}",
                        fastled_dir.join("src/platforms/wasm/compiler").display()
                    ),
                    format!("--js-library={}", js_library.display()),
                    "--pre-js".to_string(),
                    loader.display().to_string(),
                    "-sMAIN_MODULE=1".to_string(),
                    "-sAUTOLOAD_DYLIBS=0".to_string(),
                    "-o".to_string(),
                    cached_js.display().to_string(),
                ];
                main_args.extend(main_link_flags.iter().cloned());
                main_args.extend([
                    "-sINCLUDE_FULL_LIBRARY=1".to_string(),
                    "-sFILESYSTEM=1".to_string(),
                    "-sAUTO_NATIVE_LIBRARIES=1".to_string(),
                    "-sERROR_ON_UNDEFINED_SYMBOLS=0".to_string(),
                    "-sEXPORT_ALL=1".to_string(),
                    "-Wl,--export-dynamic".to_string(),
                    "-Wl,--export-all".to_string(),
                    wasm_bigint_flag.clone(),
                ]);
                log("[WASM] Linking cached dynamic runtime...", "stdout");
                run_em_link_with_retries(fastled_dir, tools, &main_args, log)?;
                let mut artifacts = vec!["fastled.js", "fastled.wasm"];
                if staging.path().join("fastled.wasm.map").is_file() {
                    artifacts.push("fastled.wasm.map");
                }
                if staging.path().join("fastled.js.map").is_file() {
                    artifacts.push("fastled.js.map");
                }
                fs::remove_file(loader).ok();
                dynamic_cache::write_metadata(staging.path(), &runtime_fingerprint, &artifacts)?;
                dynamic_cache::publish_staging(staging, &runtime_entry)?;
                dynamic_cache::validate_entry(
                    &runtime_entry,
                    &runtime_fingerprint,
                    &["fastled.js", "fastled.wasm"],
                )
                .map_err(anyhow::Error::msg)
            })();
            if let Err(error) = rebuild {
                if let Err(mark_error) = dynamic_cache::mark_failure(
                    &runtime_root,
                    &runtime_fingerprint,
                    "main-link",
                    &error,
                ) {
                    log(
                        &format!(
                            "[WASM] warning: failed to record main-link failure: {mark_error:#}"
                        ),
                        "stderr",
                    );
                }
                return Err(error);
            }
            dynamic_cache::clear_attempt(&runtime_root, &runtime_fingerprint)?;
            log(
                &format!(
                    "[WASM] Dynamic runtime published: {} ({:.2}s)",
                    &runtime_fingerprint[..12],
                    runtime_phase_start.elapsed().as_secs_f64()
                ),
                "stdout",
            );
        }
    }
    drop(_runtime_lock);

    let include_flags = vec![
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
    ];

    let sketch_object_record =
        fs::read(sketch_object).with_context(|| format!("read {}", sketch_object.display()))?;
    let direct_side_link = direct_side_link_supported(tools, mode, &wasm_bigint_flag);
    let side_link_strategy = if direct_side_link {
        "direct-emscripten-4.0.19-v1"
    } else {
        "empp-driver-v1"
    };
    let sketch_fingerprint = dynamic_cache::fingerprint_values([
        runtime_fingerprint.as_bytes(),
        toolchain_fingerprint.as_bytes(),
        sketch.fingerprint.as_bytes(),
        sketch_object_record.as_slice(),
        side_module_link_flags.join("\n").as_bytes(),
        b"SIDE_MODULE=1",
        side_link_strategy.as_bytes(),
    ]);
    let sketch_root = sketch_cache_dir.join("dynamic-sketch-cache");
    let sketch_entry = dynamic_cache::entry_path(&sketch_root, &sketch_fingerprint);
    let sketch_phase_start = std::time::Instant::now();
    let _sketch_lock = dynamic_cache::CacheLock::acquire(&sketch_root, &sketch_fingerprint)?;
    match dynamic_cache::validate_entry(&sketch_entry, &sketch_fingerprint, &["sketch.wasm"]) {
        Ok(()) => {
            dynamic_cache::clear_attempt(&sketch_root, &sketch_fingerprint)?;
            log(
                &format!(
                    "[WASM] Dynamic sketch cache hit: {} ({:.2}s)",
                    &sketch_fingerprint[..12],
                    sketch_phase_start.elapsed().as_secs_f64()
                ),
                "stdout",
            );
        }
        Err(reason) => {
            let prior = dynamic_cache::previous_attempt(&sketch_root, &sketch_fingerprint)
                .map(|attempt| format!("; previous attempt: {attempt}"))
                .unwrap_or_default();
            log(
                &format!(
                    "[WASM] Dynamic sketch cache miss: {} ({reason}{prior})",
                    &sketch_fingerprint[..12],
                ),
                "stdout",
            );
            dynamic_cache::mark_pending(&sketch_root, &sketch_fingerprint, "side-link")?;
            let rebuild = (|| -> Result<()> {
                let staging = dynamic_cache::staging_dir(&sketch_root, ".sketch-staging-")?;
                let cached_sketch = staging.path().join("sketch.wasm");
                let mut side_args = vec![sketch_object.display().to_string()];
                side_args.extend(include_flags);
                side_args.extend(["-pthread".to_string(), "-sSIDE_MODULE=1".to_string()]);
                side_args.extend(side_module_link_flags);
                side_args.extend(["-o".to_string(), cached_sketch.display().to_string()]);
                if direct_side_link {
                    log(
                        "[WASM] Linking sketch side module directly (Emscripten 4.0.19 quick plan)...",
                        "stdout",
                    );
                    if let Err(error) = run_direct_side_link_4019(
                        tools,
                        sketch_object,
                        &cached_sketch,
                        fastled_dir,
                        log,
                    ) {
                        log(
                            &format!(
                                "[WASM] Direct side link failed ({error:#}); falling back to em++"
                            ),
                            "stderr",
                        );
                        fs::remove_file(&cached_sketch).ok();
                        run_em_link_with_retries(fastled_dir, tools, &side_args, log)?;
                    }
                } else {
                    log(
                        "[WASM] Linking cached sketch side module with em++...",
                        "stdout",
                    );
                    run_em_link_with_retries(fastled_dir, tools, &side_args, log)?;
                }
                dynamic_cache::write_metadata(
                    staging.path(),
                    &sketch_fingerprint,
                    &["sketch.wasm"],
                )?;
                dynamic_cache::publish_staging(staging, &sketch_entry)?;
                dynamic_cache::validate_entry(&sketch_entry, &sketch_fingerprint, &["sketch.wasm"])
                    .map_err(anyhow::Error::msg)
            })();
            if let Err(error) = rebuild {
                if let Err(mark_error) = dynamic_cache::mark_failure(
                    &sketch_root,
                    &sketch_fingerprint,
                    "side-link",
                    &error,
                ) {
                    log(
                        &format!(
                            "[WASM] warning: failed to record side-link failure: {mark_error:#}"
                        ),
                        "stderr",
                    );
                }
                return Err(error);
            }
            dynamic_cache::clear_attempt(&sketch_root, &sketch_fingerprint)?;
            log(
                &format!(
                    "[WASM] Dynamic sketch published: {} ({:.2}s)",
                    &sketch_fingerprint[..12],
                    sketch_phase_start.elapsed().as_secs_f64()
                ),
                "stdout",
            );
        }
    }

    let output_phase_start = std::time::Instant::now();
    let copied = copy_dynamic_output(&runtime_entry, &sketch_entry, output_js)?;
    log(
        &format!(
            "[WASM] Dynamic output publication: {copied} changed artifact(s) ({:.2}s)",
            output_phase_start.elapsed().as_secs_f64()
        ),
        "stdout",
    );
    Ok(())
}

fn toolchain_fingerprint(tools: &ToolPaths) -> Result<String> {
    let install_root = tools
        .emscripten_dir
        .parent()
        .unwrap_or(&tools.emscripten_dir);
    let binary_suffix = if cfg!(windows) { ".exe" } else { "" };
    let include = [
        "emscripten/emcc.py",
        "emscripten/em++.py",
        "emscripten/emar.py",
        "emscripten/emscripten-version.txt",
        "lib/clang/**/*",
        ".emscripten",
    ];
    let content = dynamic_cache::fingerprint_tree(install_root, &include, &[])?;
    let clang = install_root.join(format!("bin/clang{binary_suffix}"));
    let wasm_ld = install_root.join(format!("bin/wasm-ld{binary_suffix}"));
    let llvm_ar = install_root.join(format!("bin/llvm-ar{binary_suffix}"));
    let mut values = vec![
        format!("content={content}"),
        format!("python={}", tools.python.display()),
        format!("clang={}", clang.display()),
        format!("wasm-ld={}", wasm_ld.display()),
        format!("llvm-ar={}", llvm_ar.display()),
        format!("target={}-{}", std::env::consts::ARCH, std::env::consts::OS),
    ];
    values.extend(
        build_env(tools)
            .into_iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    Ok(dynamic_cache::fingerprint_values(
        values.iter().map(|value| value.as_bytes()),
    ))
}

fn dynamic_wasm_bigint_flag(link_flags: &[String]) -> String {
    link_flags
        .iter()
        .rev()
        .find(|flag| flag.starts_with("-sWASM_BIGINT="))
        .cloned()
        .unwrap_or_else(|| "-sWASM_BIGINT=1".to_string())
}

fn static_link_fingerprint(
    link_flags: &[String],
    sketch_fingerprint: &str,
    library_fingerprint: &str,
) -> String {
    let flag_blob = link_flags.join("\n");
    dynamic_cache::fingerprint_values([
        b"link-mode=static",
        flag_blob.as_bytes(),
        sketch_fingerprint.as_bytes(),
        library_fingerprint.as_bytes(),
    ])
}

/// Canonicalize and lexically normalize, stripping the Windows `\\?\`
/// long-path prefix that breaks external tools (meson, Python, emcc).
/// See `crate::path::canonicalize_normalized` for details.
fn normalize_path(path: &Path) -> PathBuf {
    crate::path::canonicalize_normalized(path).into_path_buf()
}

pub(crate) fn resolve_example_name(
    sketch_dir: &Path,
    fastled_dir: &Path,
) -> (String, PathBuf, bool) {
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

fn build_dir(fastled_dir: &Path, mode: BuildMode, link_mode: LinkMode) -> PathBuf {
    let suffix = match link_mode {
        LinkMode::Static => String::new(),
        LinkMode::Dynamic => "-dynamic".to_string(),
    };
    fastled_dir
        .join(".build")
        .join(format!("meson-wasm-{}{}", mode.as_str(), suffix))
}

pub(crate) fn sketch_cache_dir(example_dir: &Path) -> PathBuf {
    example_dir.join(".build").join("wasm")
}

pub(crate) fn sketch_ino_file(example_dir: &Path) -> Result<PathBuf> {
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

pub(crate) fn get_sketch_compile_flags(
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

fn compute_source_file_hash(fastled_dir: &Path) -> Result<String> {
    dynamic_cache::fingerprint_tree(
        fastled_dir,
        &[
            "src/**/*.cpp",
            "src/**/*.cc",
            "src/**/*.cxx",
            "src/**/*.c",
            "src/**/*.h",
            "src/**/*.hpp",
            "src/**/*.hxx",
            "src/**/*.ipp",
            "src/**/*.toml",
            "src/**/*.build",
            "src/platforms/wasm/compiler/js_library.js",
            "meson.build",
            "meson_options.txt",
            "ci/meson/**/meson.build",
            "ci/meson/**/*.py",
            "ci/wasm_flags.py",
            "ci/meson/wasm_cross_file.ini",
        ],
        &[".git/**", ".build/**", "build/**", "fastled_js/**"],
    )
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

#[allow(clippy::too_many_arguments)]
fn ensure_meson_configured(
    fastled_dir: &Path,
    tools: &ToolPaths,
    build_dir: &Path,
    mode: BuildMode,
    link_mode: LinkMode,
    force: bool,
    source_fingerprint: &str,
    log: LogSink,
) -> Result<()> {
    let marker = build_dir.join(".src_file_list_hash");
    let current_hash = source_fingerprint;
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
    if link_mode == LinkMode::Dynamic {
        command.env("CFLAGS", "-fPIC");
        command.env("CXXFLAGS", "-fPIC");
    }
    log(
        &format!("[WASM] Configuring meson (mode: {})...", mode.as_str()),
        "stdout",
    );
    run_status(command, "meson setup", log)?;
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

fn library_archive_is_valid(path: &Path) -> bool {
    let mut magic = [0_u8; 8];
    path.metadata().is_ok_and(|metadata| metadata.len() > 8)
        && fs::File::open(path)
            .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut magic))
            .is_ok()
        && (magic == *b"!<arch>\n" || magic == *b"!<thin>\n")
}

fn build_library(
    fastled_dir: &Path,
    tools: &ToolPaths,
    build_dir: &Path,
    source_fingerprint: &str,
    log: LogSink,
) -> Result<bool> {
    let archive = library_archive(build_dir);
    let fingerprint_path = build_dir.join("library_src_fingerprint");
    let current = source_fingerprint;
    if library_archive_is_valid(&archive)
        && fingerprint_path.is_file()
        && fs::read_to_string(&fingerprint_path)
            .unwrap_or_default()
            .trim()
            == current
    {
        log("[WASM] Library up-to-date", "stdout");
        return Ok(false);
    }
    if archive.exists() && !library_archive_is_valid(&archive) {
        log(
            "[WASM] Invalid or truncated libfastled.a; rebuilding library",
            "stdout",
        );
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
    log("[WASM] Building libfastled.a...", "stdout");
    run_status(command, "meson compile fastled", log)?;
    if !library_archive_is_valid(&archive) {
        bail!(
            "meson reported success but produced an invalid archive at {}",
            archive.display()
        );
    }
    fs::write(fingerprint_path, current)?;
    log("[WASM] Library build successful", "stdout");
    Ok(true)
}

pub(crate) fn create_wrapper(
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

#[allow(clippy::too_many_arguments)]
fn build_sketch_pch(
    fastled_dir: &Path,
    tools: &ToolPaths,
    build_dir: &Path,
    mode: BuildMode,
    emsdk_root: Option<&Path>,
    lib_was_rebuilt: bool,
    source_fingerprint: &str,
    log: LogSink,
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
    hash_input.push_str(source_fingerprint);
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

    log("[WASM] Building sketch header unit (.pcm)...", "stdout");
    let mut command = command_with_env(&tools.python, tools);
    command
        .current_dir(fastled_dir)
        .arg(&tools.emcc)
        .args(&args);
    run_status(command, "emcc header unit", log)?;

    let mut command = command_with_env(&tools.python, tools);
    command
        .current_dir(fastled_dir)
        .arg(&tools.emcc)
        .args(["-c"])
        .arg(&pcm)
        .args(["-o"])
        .arg(&pch_o)
        .args(["-O0", "-g0"]);
    if run_status(command, "emcc header unit companion", log).is_ok() {
        let archive = library_archive(build_dir);
        if archive.is_file() && tools.emar.is_file() {
            let mut command = command_with_env(&tools.python, tools);
            command
                .current_dir(fastled_dir)
                .arg(&tools.emar)
                .arg("r")
                .arg(&archive)
                .arg(&pch_o);
            let _ = run_status(command, "emar pch companion", log);
        }
    }

    fs::write(hash_path, current_hash)?;
    Ok(Some(pcm))
}

#[allow(clippy::too_many_arguments)]
fn compile_sketch(
    tools: &ToolPaths,
    wrapper: &Path,
    build_dir: &Path,
    sketch_cache_dir: &Path,
    example_dir: &Path,
    mode: BuildMode,
    link_mode: LinkMode,
    dwarf_roots: &DwarfPathRoots,
    fingerprints: &BuildFingerprints,
    log: LogSink,
) -> Result<CompiledSketch> {
    let compile_phase_start = std::time::Instant::now();
    let fastled_dir = &dwarf_roots.fastled_dir;
    let compile_flags = get_sketch_compile_flags(fastled_dir, mode, Some(dwarf_roots))?;
    let fastled_fingerprint = &fingerprints.fastled_source;
    let toolchain_fingerprint = &fingerprints.toolchain;
    let mut driver_args = compile_flags.clone();
    if link_mode == LinkMode::Dynamic {
        driver_args.push("-fPIC".to_string());
    }
    let (direct_backend_flags, direct_cflags_error, backend_identity) =
        match direct_clang_cflags(tools, toolchain_fingerprint, &driver_args, fastled_dir) {
            Ok(flags) => {
                let identity =
                    dynamic_cache::fingerprint_values(flags.iter().map(|flag| flag.as_bytes()));
                (Some(flags), None, format!("direct-clang-v1:{identity}"))
            }
            Err(error) => (
                None,
                Some(format!("{error:#}")),
                "empp-fallback-v1".to_string(),
            ),
        };
    let source_fingerprint = dynamic_cache::fingerprint_tree(
        example_dir,
        &[
            "**/*.ino", "**/*.cpp", "**/*.c", "**/*.h", "**/*.hpp", "**/*.ipp",
        ],
        &[".git/**", ".build/**", "fastled_js/**"],
    )?;
    let wrapper_contents = fs::read(wrapper)
        .with_context(|| format!("read generated wrapper {}", wrapper.display()))?;
    let compile_flag_blob = compile_flags.join("\n");
    let link_mode_value = format!("link-mode={link_mode:?}");
    let object_fingerprint = dynamic_cache::fingerprint_values([
        source_fingerprint.as_bytes(),
        fastled_fingerprint.as_bytes(),
        toolchain_fingerprint.as_bytes(),
        compile_flag_blob.as_bytes(),
        link_mode_value.as_bytes(),
        wrapper_contents.as_slice(),
        backend_identity.as_bytes(),
    ]);
    let object_root = sketch_cache_dir.join("object-cache");
    let object_entry = dynamic_cache::entry_path(&object_root, &object_fingerprint);
    let object = object_entry.join("sketch.o");
    let _lock = dynamic_cache::CacheLock::acquire(&object_root, &object_fingerprint)?;
    if dynamic_cache::validate_entry(&object_entry, &object_fingerprint, &["sketch.o"]).is_ok() {
        log(
            &format!(
                "[WASM] Sketch object cache hit: {} ({:.2}s)",
                &object_fingerprint[..12],
                compile_phase_start.elapsed().as_secs_f64()
            ),
            "stdout",
        );
        return Ok(CompiledSketch {
            object,
            fingerprint: object_fingerprint,
        });
    }

    let staging = dynamic_cache::staging_dir(&object_root, ".object-staging-")?;
    let staging_object = staging.path().join("sketch.o");

    let mut args = vec![
        "-c".to_string(),
        wrapper.display().to_string(),
        "-o".to_string(),
        staging_object.display().to_string(),
    ];
    args.extend(compile_flags.iter().cloned());
    if link_mode == LinkMode::Dynamic {
        args.push("-fPIC".to_string());
    }
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

    log(
        &format!("[WASM] Compiling sketch: {}", wrapper.display()),
        "stdout",
    );
    let direct_result = if let Some(backend_flags) = direct_backend_flags {
        let mut command = command_with_env(&tools.clangpp, tools);
        command
            .current_dir(fastled_dir)
            .args(backend_flags)
            .args(&args);
        run_status(command, "clang++ sketch compile", log)
    } else {
        Err(anyhow::anyhow!(
            "{}",
            direct_cflags_error.unwrap_or_else(|| "em++ --cflags unavailable".to_string())
        ))
    };
    if let Err(direct_error) = direct_result {
        log(
            &format!(
                "[WASM] Direct clang compile unavailable ({direct_error:#}); falling back to em++"
            ),
            "stderr",
        );
        fs::remove_file(&staging_object).ok();
        let mut command = command_with_env(&tools.python, tools);
        command
            .current_dir(fastled_dir)
            .arg(&tools.empp)
            .args(&args);
        run_status(command, "em++ sketch compile fallback", log)?;
    }
    dynamic_cache::write_metadata(staging.path(), &object_fingerprint, &["sketch.o"])?;
    dynamic_cache::publish_staging(staging, &object_entry)?;
    dynamic_cache::validate_entry(&object_entry, &object_fingerprint, &["sketch.o"])
        .map_err(anyhow::Error::msg)?;
    log(
        &format!(
            "[WASM] Sketch compile published: {} ({:.2}s)",
            &object_fingerprint[..12],
            compile_phase_start.elapsed().as_secs_f64()
        ),
        "stdout",
    );
    Ok(CompiledSketch {
        object,
        fingerprint: object_fingerprint,
    })
}

#[allow(clippy::too_many_arguments)]
fn link_wasm(
    fastled_dir: &Path,
    tools: &ToolPaths,
    sketch: &CompiledSketch,
    build_dir: &Path,
    sketch_cache_dir: &Path,
    output_js: &Path,
    mode: BuildMode,
    link_mode: LinkMode,
    fingerprints: &BuildFingerprints,
    log: LogSink,
) -> Result<()> {
    if link_mode == LinkMode::Dynamic {
        return link_wasm_dynamic(
            fastled_dir,
            tools,
            sketch,
            build_dir,
            sketch_cache_dir,
            output_js,
            mode,
            fingerprints,
            log,
        );
    }
    let sketch_object = &sketch.object;
    let archive = library_archive(build_dir);
    let cached_js = sketch_cache_dir.join("fastled.js");
    let cached_wasm = sketch_cache_dir.join("fastled.wasm");
    // Static and dynamic modes share the sketch cache directory. Never let a
    // side module from a previous dynamic build leak into static output.
    let cached_sketch = sketch_cache_dir.join("sketch.wasm");
    if cached_sketch.is_file() {
        fs::remove_file(&cached_sketch)
            .with_context(|| format!("remove stale {}", cached_sketch.display()))?;
    }
    let link_flags = get_link_flags(fastled_dir, mode)?;
    let flags_hash_path = sketch_cache_dir.join("link_flags.hash");
    let current_flags_hash =
        static_link_fingerprint(&link_flags, &sketch.fingerprint, &fingerprints.library);
    if cached_js.is_file()
        && cached_wasm.is_file()
        && fs::read_to_string(&flags_hash_path)
            .unwrap_or_default()
            .trim()
            == current_flags_hash
    {
        copy_linked_output(sketch_cache_dir, output_js)?;
        log("[WASM] Link output up-to-date", "stdout");
        return Ok(());
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

    log("[WASM] Linking final WASM module...", "stdout");
    run_em_link_with_retries(fastled_dir, tools, &args, log)?;
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
fn run_em_link_with_retries(
    fastled_dir: &Path,
    tools: &ToolPaths,
    args: &[String],
    log: LogSink,
) -> Result<()> {
    let max_attempts = if cfg!(windows) { 6 } else { 1 };
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=max_attempts {
        let mut command = command_with_env(&tools.python, tools);
        command.current_dir(fastled_dir).arg(&tools.empp).args(args);
        match run_status(command, "em++ wasm link", log) {
            Ok(()) => return Ok(()),
            Err(err) => {
                if attempt < max_attempts {
                    log(
                        &format!(
                            "[WASM] em++ link attempt {attempt}/{max_attempts} failed; \
                             retrying (sysroot libs cache progressively): {err:#}"
                        ),
                        "stdout",
                    );
                }
                last_err = Some(err);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("em++ wasm link failed")))
}

fn direct_side_link_supported(tools: &ToolPaths, mode: BuildMode, wasm_bigint: &str) -> bool {
    mode == BuildMode::Quick
        && wasm_bigint == "-sWASM_BIGINT=0"
        && tools.emscripten_version == "4.0.19"
        && tools.wasm_ld.is_file()
        && tools.llvm_objcopy.is_file()
        && tools.wasm_finalize.is_file()
        && std::env::var_os("FASTLED_DISABLE_DIRECT_SIDE_LINK").is_none()
}

/// Version-pinned quick-mode side-link plan matching Emscripten 4.0.19's
/// final (post metadata-discovery) SIDE_MODULE=1 wasm-ld invocation. Unknown
/// versions and configurations never enter this path.
fn direct_side_link_args(tools: &ToolPaths, object: &Path, output: &Path) -> Vec<String> {
    let pic_lib = tools
        .emscripten_dir
        .join("cache/sysroot/lib/wasm32-emscripten/pic");
    vec![
        "-o".to_string(),
        output.display().to_string(),
        "--whole-archive".to_string(),
        object.display().to_string(),
        format!("-L{}", pic_lib.display()),
        format!("-L{}", tools.emscripten_dir.join("src/lib").display()),
        pic_lib.join("crtbegin.o").display().to_string(),
        "--no-whole-archive".to_string(),
        "-mllvm".to_string(),
        "-combiner-global-alias-analysis=false".to_string(),
        "-mllvm".to_string(),
        "-enable-emscripten-sjlj".to_string(),
        "-mllvm".to_string(),
        "-disable-lsr".to_string(),
        "--import-memory".to_string(),
        "--shared-memory".to_string(),
        "--strip-debug".to_string(),
        "--export-dynamic".to_string(),
        "--export=__wasm_call_ctors".to_string(),
        "--export=_emscripten_tls_init".to_string(),
        "--export-if-defined=__start_em_asm".to_string(),
        "--export-if-defined=__stop_em_asm".to_string(),
        "--export-if-defined=__start_em_lib_deps".to_string(),
        "--export-if-defined=__stop_em_lib_deps".to_string(),
        "--export-if-defined=__start_em_js".to_string(),
        "--export-if-defined=__stop_em_js".to_string(),
        "--export-if-defined=main".to_string(),
        "--export-if-defined=__main_argc_argv".to_string(),
        "--export-if-defined=fflush".to_string(),
        "--experimental-pic".to_string(),
        "--unresolved-symbols=import-dynamic".to_string(),
        "--no-shlib-sigcheck".to_string(),
        "-shared".to_string(),
        "--stack-first".to_string(),
    ]
}

fn run_direct_side_link_4019(
    tools: &ToolPaths,
    object: &Path,
    output: &Path,
    current_dir: &Path,
    log: LogSink,
) -> Result<()> {
    let mut linker = command_with_env(&tools.wasm_ld, tools);
    linker
        .current_dir(current_dir)
        .args(direct_side_link_args(tools, object, output));
    run_status(linker, "direct wasm-ld side link", log)?;

    let mut objcopy = command_with_env(&tools.llvm_objcopy, tools);
    objcopy
        .current_dir(current_dir)
        .arg(output)
        .arg(output)
        .arg("--remove-section=.debug*")
        .arg("--remove-section=producers")
        .arg("--remove-section=name");
    run_status(objcopy, "direct side-link strip", log)?;

    let mut finalize = command_with_env(&tools.wasm_finalize, tools);
    finalize
        .current_dir(current_dir)
        .arg("--dyncalls-i64")
        .arg("--pass-arg=legalize-js-interface-export-originals")
        .arg("--side-module")
        .arg(output)
        .arg("-o")
        .arg(output)
        .arg("--detect-features");
    run_status(finalize, "direct side-link finalize", log)
}

fn copy_linked_output(sketch_cache_dir: &Path, output_js: &Path) -> Result<()> {
    let output_dir = output_js
        .parent()
        .ok_or_else(|| anyhow::anyhow!("output path has no parent: {}", output_js.display()))?;
    fs::create_dir_all(output_dir)?;
    for name in [
        "fastled.js",
        "fastled.wasm",
        "sketch.wasm",
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

fn files_equal(left: &Path, right: &Path) -> Result<bool> {
    use std::io::Read;

    let left_meta = fs::metadata(left)?;
    let right_meta = match fs::metadata(right) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if left_meta.len() != right_meta.len() {
        return Ok(false);
    }
    let mut left = fs::File::open(left)?;
    let mut right = fs::File::open(right)?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn copy_file_if_changed(source: &Path, target: &Path) -> Result<bool> {
    if files_equal(source, target)? {
        return Ok(false);
    }
    fs::copy(source, target)
        .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
    Ok(true)
}

fn copy_dynamic_output(
    runtime_entry: &Path,
    sketch_entry: &Path,
    output_js: &Path,
) -> Result<usize> {
    let output_dir = output_js
        .parent()
        .ok_or_else(|| anyhow::anyhow!("output path has no parent: {}", output_js.display()))?;
    fs::create_dir_all(output_dir)?;
    let mut copied = 0;
    for (source_dir, name) in [
        (runtime_entry, "fastled.js"),
        (runtime_entry, "fastled.wasm"),
        (runtime_entry, "fastled.wasm.map"),
        (runtime_entry, "fastled.js.map"),
        (sketch_entry, "sketch.wasm"),
    ] {
        let source = source_dir.join(name);
        let target = output_dir.join(name);
        if source.is_file() {
            if copy_file_if_changed(&source, &target)? {
                copied += 1;
            }
        } else if target.exists() {
            fs::remove_file(&target)
                .with_context(|| format!("remove stale {}", target.display()))?;
        }
    }
    Ok(copied)
}

fn generate_manifest(example_dir: &Path, output_dir: &Path) -> Result<()> {
    let mut files = Vec::<serde_json::Value>::new();
    collect_data_files(example_dir, example_dir, &mut files)?;
    let legacy = output_dir.join("files.json");
    if legacy.is_file() {
        fs::remove_file(&legacy)
            .with_context(|| format!("remove legacy asset manifest {}", legacy.display()))?;
    }
    fs::write(
        output_dir.join("sketch_assets.json"),
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

/// Resolve the FastLED checkout for a sketch directory without a full
/// `BuildRequest`. Used by `fastled --write-clangd` to mirror the build's
/// FastLED-ref resolution (honours the `fastled.json` pin).
pub(crate) fn resolve_fastled_dir_for_sketch(sketch_dir: &Path) -> Result<PathBuf> {
    let request = BuildRequest {
        sketch_dir: sketch_dir.to_path_buf(),
        build_mode: BuildMode::Quick,
        profile: false,
        fastled_path: None,
        force_clean: false,
        emit_clangd: false,
        no_app: false,
        link_mode: LinkMode::Static,
    };
    let (fastled_dir, _cleanup) = resolve_fastled_dir(&request)?;
    Ok(normalize_path(&fastled_dir))
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

/// Run the build, streaming all tool output to stdout/stderr.
pub fn run_build(request: &BuildRequest) -> Result<BuildResult> {
    run_build_streaming(request, &stdio_log)
}

/// Run the build, forwarding every output line (tool stdout/stderr plus
/// `[WASM]` progress markers) to `log` as it is produced.
pub fn run_build_streaming(request: &BuildRequest, log: LogSink) -> Result<BuildResult> {
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
    let emsdk_root = Some(emscripten_install.clone());
    let (example_name, example_dir, _is_in_tree) =
        resolve_example_name(&normalize_path(&request.sketch_dir), &fastled_dir);

    let sketch_start = std::time::Instant::now();
    let strategy = if output_dir.join("fastled.js").is_file()
        && output_dir.join("fastled.wasm").is_file()
        && (request.link_mode == LinkMode::Static || output_dir.join("sketch.wasm").is_file())
        && !request.force_clean
    {
        match request.link_mode {
            LinkMode::Static => "incremental-static",
            LinkMode::Dynamic => "incremental-dynamic",
        }
    } else {
        "cold"
    }
    .to_string();

    let build_result = (|| -> Result<()> {
        let build_dir = build_dir(&fastled_dir, request.build_mode, request.link_mode);
        let sketch_cache = sketch_cache_dir(&example_dir);
        if request.force_clean {
            for cache in [
                build_dir.join("dynamic-runtime-cache"),
                sketch_cache.join("dynamic-sketch-cache"),
                sketch_cache.join("object-cache"),
            ] {
                if cache.exists() {
                    fs::remove_dir_all(&cache)
                        .with_context(|| format!("purge {}", cache.display()))?;
                }
            }
        }
        let fingerprint_start = std::time::Instant::now();
        let fingerprints = BuildFingerprints::compute(
            &fastled_dir,
            &tools,
            request.build_mode,
            request.link_mode,
        )?;
        log(
            &format!(
                "[WASM] Build fingerprints computed ({:.2}s; FastLED {:.2}s, toolchain {:.2}s, parallel)",
                fingerprint_start.elapsed().as_secs_f64(),
                fingerprints.source_duration_secs,
                fingerprints.toolchain_duration_secs,
            ),
            "stdout",
        );
        ensure_meson_configured(
            &fastled_dir,
            &tools,
            &build_dir,
            request.build_mode,
            request.link_mode,
            request.force_clean,
            &fingerprints.library,
            log,
        )?;
        let lib_rebuilt =
            build_library(&fastled_dir, &tools, &build_dir, &fingerprints.library, log)?;
        let _ = build_sketch_pch(
            &fastled_dir,
            &tools,
            &build_dir,
            request.build_mode,
            emsdk_root.as_deref(),
            lib_rebuilt,
            &fingerprints.library,
            log,
        );
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
            request.link_mode,
            &sketch_dwarf_roots,
            &fingerprints,
            log,
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
            request.link_mode,
            &fingerprints,
            log,
        )?;
        if request.no_app {
            frontend::remove_app_from_output(&output_dir)?;
        } else {
            frontend::copy_frontend_to_output(&output_dir, None)?;
        }
        generate_manifest(&example_dir, &output_dir)?;
        let debug_config = debug_symbols::load_debug_symbol_config(
            example_dir.clone(),
            Some(fastled_dir.clone()),
            emsdk_root.clone(),
        );
        debug_symbols::write_debug_symbol_manifest(&output_dir, &debug_config)?;

        // Emit clangd/VS Code configuration so "Go to Definition" works for
        // the sketch in VS Code (Refs #177). Opt-in via `--clangd` (Refs
        // #179): writing IDE config into the user's project by default is
        // too intrusive. Non-fatal: a config-write failure must never fail
        // an otherwise successful build.
        if request.emit_clangd {
            let clangd_result = (|| -> Result<()> {
                let ino_file = sketch_ino_file(&example_dir)?;
                let compile_flags = get_sketch_compile_flags(
                    &fastled_dir,
                    request.build_mode,
                    Some(&sketch_dwarf_roots),
                )?;
                crate::clangd_config::write_clangd_config(
                    &crate::clangd_config::ClangdConfigInputs {
                        sketch_dir: crate::path::NormalizedPath::new(&example_dir),
                        fastled_dir: crate::path::NormalizedPath::new(&fastled_dir),
                        emsdk_install_dir: crate::path::NormalizedPath::new(&emscripten_install),
                        tools_emcc_path: crate::path::NormalizedPath::new(&tools.emcc),
                        tools_empp_path: crate::path::NormalizedPath::new(&tools.empp),
                        wrapper_source: crate::path::NormalizedPath::new(&wrapper),
                        ino_file: crate::path::NormalizedPath::new(ino_file),
                        compile_flags,
                        build_mode: request.build_mode,
                    },
                )
            })();
            if let Err(err) = clangd_result {
                log(
                    &format!("[WASM] warning: failed to write clangd config: {err:#}"),
                    "stderr",
                );
            }
        }
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

    fn shell_command(script: &str) -> Command {
        if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", script]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            command
        }
    }

    fn collect_run_status_lines(script: &str) -> (Result<()>, Vec<(String, String)>) {
        let lines = std::cell::RefCell::new(Vec::new());
        let log = |line: &str, stream: &str| {
            lines
                .borrow_mut()
                .push((line.to_string(), stream.to_string()));
        };
        let result = run_status(shell_command(script), "test command", &log);
        (result, lines.into_inner())
    }

    #[test]
    fn run_status_streams_stdout_and_stderr_lines() {
        let script = if cfg!(windows) {
            // No space before the redirect: cmd's echo would include it.
            "echo out-line& echo err-line>&2"
        } else {
            "echo out-line; echo err-line >&2"
        };
        let (result, lines) = collect_run_status_lines(script);
        result.unwrap();
        assert!(
            lines.contains(&("out-line".to_string(), "stdout".to_string())),
            "missing stdout line, got: {lines:?}"
        );
        assert!(
            lines.contains(&("err-line".to_string(), "stderr".to_string())),
            "missing stderr line, got: {lines:?}"
        );
    }

    #[test]
    fn run_status_reports_failure_but_still_streams_output() {
        let script = if cfg!(windows) {
            "echo doomed& exit 3"
        } else {
            "echo doomed; exit 3"
        };
        let (result, lines) = collect_run_status_lines(script);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("test command failed"), "got: {err}");
        assert!(
            lines.contains(&("doomed".to_string(), "stdout".to_string())),
            "output before failure must still stream, got: {lines:?}"
        );
    }

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
    fn dynamic_linking_makes_wasm_bigint_abi_explicit() {
        assert_eq!(
            dynamic_wasm_bigint_flag(&["-sWASM_BIGINT=0".to_string()]),
            "-sWASM_BIGINT=0"
        );
        assert_eq!(
            dynamic_wasm_bigint_flag(&[
                "-sWASM_BIGINT=1".to_string(),
                "-sWASM_BIGINT=0".to_string(),
            ]),
            "-sWASM_BIGINT=0"
        );
        assert_eq!(dynamic_wasm_bigint_flag(&[]), "-sWASM_BIGINT=1");
    }

    #[test]
    fn static_link_fingerprint_tracks_sketch_library_and_flags() {
        let baseline = static_link_fingerprint(&["-O0".to_string()], "sketch-a", "library-a");
        assert_ne!(
            baseline,
            static_link_fingerprint(&["-O1".to_string()], "sketch-a", "library-a")
        );
        assert_ne!(
            baseline,
            static_link_fingerprint(&["-O0".to_string()], "sketch-b", "library-a")
        );
        assert_ne!(
            baseline,
            static_link_fingerprint(&["-O0".to_string()], "sketch-a", "library-b")
        );
    }

    #[test]
    fn direct_cflags_key_tracks_toolchain_and_driver_arguments() {
        let baseline = direct_cflags_key("toolchain-a", &["-pthread".to_string()]);
        assert_eq!(
            baseline,
            direct_cflags_key("toolchain-a", &["-pthread".to_string()])
        );
        assert_ne!(
            baseline,
            direct_cflags_key("toolchain-b", &["-pthread".to_string()])
        );
        assert_ne!(
            baseline,
            direct_cflags_key("toolchain-a", &["-fPIC".to_string()])
        );
    }

    fn fake_tools_for_direct_link(root: &Path, version: &str) -> ToolPaths {
        let emscripten_dir = root.join("emscripten");
        let bin = root.join("bin");
        fs::create_dir_all(&emscripten_dir).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let wasm_ld = bin.join("wasm-ld");
        let llvm_objcopy = bin.join("llvm-objcopy");
        let wasm_finalize = bin.join("wasm-emscripten-finalize");
        for path in [&wasm_ld, &llvm_objcopy, &wasm_finalize] {
            fs::write(path, "tool").unwrap();
        }
        ToolPaths {
            emscripten_dir,
            emcc: root.join("emcc.py"),
            empp: root.join("em++.py"),
            emar: root.join("emar.py"),
            clangpp: bin.join("clang++"),
            wasm_ld,
            llvm_objcopy,
            wasm_finalize,
            emscripten_version: version.to_string(),
            python: PathBuf::from("python"),
        }
    }

    #[test]
    fn direct_side_link_is_strictly_version_mode_and_abi_gated() {
        let temp = tempfile::tempdir().unwrap();
        let tools = fake_tools_for_direct_link(temp.path(), "4.0.19");
        assert!(direct_side_link_supported(
            &tools,
            BuildMode::Quick,
            "-sWASM_BIGINT=0"
        ));
        assert!(!direct_side_link_supported(
            &tools,
            BuildMode::Debug,
            "-sWASM_BIGINT=0"
        ));
        assert!(!direct_side_link_supported(
            &tools,
            BuildMode::Quick,
            "-sWASM_BIGINT=1"
        ));
        let unknown = fake_tools_for_direct_link(&temp.path().join("unknown"), "4.0.20");
        assert!(!direct_side_link_supported(
            &unknown,
            BuildMode::Quick,
            "-sWASM_BIGINT=0"
        ));
    }

    #[test]
    fn direct_side_link_plan_preserves_side_module_one_contract() {
        let temp = tempfile::tempdir().unwrap();
        let tools = fake_tools_for_direct_link(temp.path(), "4.0.19");
        let args = direct_side_link_args(&tools, Path::new("sketch.o"), Path::new("sketch.wasm"));
        assert!(args.contains(&"--whole-archive".to_string()));
        assert!(args.contains(&"--unresolved-symbols=import-dynamic".to_string()));
        assert!(args.contains(&"-shared".to_string()));
        assert!(!args.iter().any(|arg| arg.contains("SIDE_MODULE=2")));
    }

    #[test]
    fn dynamic_output_does_not_recopy_unchanged_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join("runtime");
        let sketch = temp.path().join("sketch");
        let output = temp.path().join("output");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&sketch).unwrap();
        fs::write(runtime.join("fastled.js"), "loader").unwrap();
        fs::write(runtime.join("fastled.wasm"), b"runtime-wasm").unwrap();
        fs::write(sketch.join("sketch.wasm"), b"sketch-v1").unwrap();

        assert_eq!(
            copy_dynamic_output(&runtime, &sketch, &output.join("fastled.js")).unwrap(),
            3
        );
        let js_mtime = output
            .join("fastled.js")
            .metadata()
            .unwrap()
            .modified()
            .unwrap();
        let wasm_mtime = output
            .join("fastled.wasm")
            .metadata()
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        assert_eq!(
            copy_dynamic_output(&runtime, &sketch, &output.join("fastled.js")).unwrap(),
            0
        );
        assert_eq!(
            output
                .join("fastled.js")
                .metadata()
                .unwrap()
                .modified()
                .unwrap(),
            js_mtime
        );
        assert_eq!(
            output
                .join("fastled.wasm")
                .metadata()
                .unwrap()
                .modified()
                .unwrap(),
            wasm_mtime
        );

        fs::write(sketch.join("sketch.wasm"), b"sketch-v2").unwrap();
        assert_eq!(
            copy_dynamic_output(&runtime, &sketch, &output.join("fastled.js")).unwrap(),
            1
        );
        assert_eq!(
            output
                .join("fastled.js")
                .metadata()
                .unwrap()
                .modified()
                .unwrap(),
            js_mtime
        );
        assert_eq!(
            output
                .join("fastled.wasm")
                .metadata()
                .unwrap()
                .modified()
                .unwrap(),
            wasm_mtime
        );
    }

    #[test]
    fn runtime_source_fingerprint_includes_js_library_and_meson_helpers() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for (relative, contents) in [
            ("src/core.cpp", "core"),
            ("src/platforms/wasm/compiler/js_library.js", "js-v1"),
            ("meson.build", "root-meson"),
            ("meson_options.txt", "options"),
            ("ci/meson/wasm/meson.build", "wasm-meson"),
            ("ci/meson/rglob.py", "rglob-v1"),
            ("ci/wasm_flags.py", "flags-v1"),
            ("ci/meson/wasm_cross_file.ini", "cross"),
        ] {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        let baseline = compute_source_file_hash(root).unwrap();
        fs::write(
            root.join("src/platforms/wasm/compiler/js_library.js"),
            "js-v2",
        )
        .unwrap();
        let js_changed = compute_source_file_hash(root).unwrap();
        assert_ne!(baseline, js_changed);

        fs::write(root.join("ci/meson/rglob.py"), "rglob-v2").unwrap();
        assert_ne!(js_changed, compute_source_file_hash(root).unwrap());
    }

    #[test]
    fn library_archive_validation_rejects_missing_empty_and_truncated_files() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("libfastled.a");
        assert!(!library_archive_is_valid(&archive));
        fs::write(&archive, []).unwrap();
        assert!(!library_archive_is_valid(&archive));
        fs::write(&archive, b"!<arch>").unwrap();
        assert!(!library_archive_is_valid(&archive));
        fs::write(&archive, b"!<arch>\nmember").unwrap();
        assert!(library_archive_is_valid(&archive));
        fs::write(&archive, b"!<thin>\nmember").unwrap();
        assert!(library_archive_is_valid(&archive));
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

    #[test]
    fn copy_linked_output_copies_dynamic_side_module_and_removes_it_when_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let output = tmp.path().join("out").join("fastled.js");
        fs::create_dir_all(&cache).unwrap();
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(cache.join("fastled.js"), "js").unwrap();
        fs::write(cache.join("fastled.wasm"), "wasm").unwrap();
        fs::write(cache.join("sketch.wasm"), "side").unwrap();

        copy_linked_output(&cache, &output).unwrap();
        assert_eq!(
            fs::read_to_string(output.parent().unwrap().join("sketch.wasm")).unwrap(),
            "side"
        );

        fs::remove_file(cache.join("sketch.wasm")).unwrap();
        copy_linked_output(&cache, &output).unwrap();
        assert!(!output.parent().unwrap().join("sketch.wasm").exists());
    }

    #[test]
    fn generate_manifest_uses_new_name_and_removes_legacy_name() {
        let tmp = tempfile::tempdir().unwrap();
        let example = tmp.path().join("Sketch");
        let output = example.join("fastled_js");
        fs::create_dir_all(example.join("data")).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(example.join("data").join("config.json"), "{}").unwrap();
        fs::write(output.join("files.json"), "legacy").unwrap();

        generate_manifest(&example, &output).unwrap();

        assert!(!output.join("files.json").exists());
        let manifest = fs::read_to_string(output.join("sketch_assets.json")).unwrap();
        assert!(manifest.contains("config.json"));
    }
}
