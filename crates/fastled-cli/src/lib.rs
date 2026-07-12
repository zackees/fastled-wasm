#![recursion_limit = "512"]

use std::process::ExitCode;

use clap::Parser;

mod archive;
mod build;
mod clangd_config;
mod cli;
mod commands;
mod compile_stream;
pub mod debug_symbols;
mod dwarf_smoke;
mod dynamic_cache;
#[cfg(test)]
mod dynamic_cache_matrix_tests;
pub mod frontend;
pub mod install;
mod install_unlock;
mod keyboard;
pub mod path;
mod paths_util;
pub mod project;
pub mod runtime;
mod selection;
mod server;
pub mod viewer;
pub mod wasm_build;
mod watcher;

pub use selection::{
    prepare_sketch_selection, resolve_prompt_choice, PromptChoice, SketchSelection,
};

const DEFAULT_EXAMPLE: &str = "wasm";

/// Library entry point invoked by the `fastled` binary.
pub fn run() -> ExitCode {
    let mut cli = cli::Cli::parse();

    if let Err(message) = cli::validate_init_ref_flags(&cli) {
        eprintln!("fastled: {message}");
        return ExitCode::FAILURE;
    }

    // Hidden plumbing for the Python side: download the FastLED repo and
    // print the local path. No further work.
    if let Some(ref ref_str) = cli.internal_ensure_fastled_repo {
        let ref_opt = if ref_str == "__latest__" {
            None
        } else {
            Some(ref_str.as_str())
        };
        return match install::ensure_fastled_repo(ref_opt) {
            Ok(path) => {
                println!("{}", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("fastled: failed to fetch FastLED repo: {e:#}");
                ExitCode::FAILURE
            }
        };
    }

    if cli.internal_dwarf_smoke {
        return commands::run_internal_dwarf_smoke(&cli);
    }

    // Handle serve-dir modes natively with the Rust HTTP server.
    if let Some((serve_dir, launch_viewer)) = serve_dir_request(&cli) {
        return commands::serve_directory(serve_dir, launch_viewer);
    }

    if cli.install {
        match install::run_install(install::InstallOptions {
            dry_run: cli.dry_run,
            no_interactive: cli.no_interactive,
        }) {
            Ok(outcome) => {
                cli.install = false;
                if !outcome.launch_after {
                    return ExitCode::SUCCESS;
                }
            }
            Err(err) => {
                eprintln!("fastled: installation failed: {err:#}");
                return ExitCode::FAILURE;
            }
        }
    }

    // Regenerate VS Code clangd configuration for a sketch without building.
    if let Some(ref dir) = cli.write_clangd {
        return match clangd_config::run_write_clangd(dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("fastled: failed to write clangd config: {err:#}");
                ExitCode::FAILURE
            }
        };
    }

    if let Some(ref init_value) = cli.init {
        return commands::run_native_init(
            &cli,
            if init_value == "__init__" {
                None
            } else {
                Some(init_value.as_str())
            },
        );
    }

    if cli.init.is_none() {
        if cli.fastled_path.is_none() {
            cli.fastled_path = paths_util::detect_local_fastled_path();
        }

        match paths_util::resolve_compile_directory(&cli) {
            Ok(Some(directory)) => cli.directory = Some(directory),
            Ok(None) => {}
            Err(message) => {
                eprintln!("fastled: {message}");
                return ExitCode::FAILURE;
            }
        }
    }

    // Normal compilation flow with Rust HTTP server + watch mode.
    // The Tauri viewer uses the same compile_and_serve() infrastructure so
    // compilation output is streamed via SSE in real time.
    //
    // Conditions:
    //  - a sketch directory was provided
    //  - the user did NOT pass --just-compile
    //  - it's not a non-compile command (--init)
    if let Some(ref dir) = cli.directory {
        if (cli.just_compile || cli.no_app) && cli.init.is_none() {
            return commands::run_native_just_compile(&cli, dir);
        }
        if !cli.just_compile && cli.init.is_none() {
            return commands::compile_and_serve(dir, &cli);
        }
    }

    eprintln!("fastled: no sketch directory specified");
    ExitCode::FAILURE
}

/// Map serve-dir CLI flags to `(directory, launch_viewer)`.
///
/// `--internal-serve-dir-headless` is the no-UI serve path: it never launches
/// a viewer, so viewer-exit shutdown can never apply to it.
fn serve_dir_request(cli: &cli::Cli) -> Option<(&str, bool)> {
    if let Some(ref dir) = cli.internal_serve_dir_headless {
        return Some((dir, false));
    }
    cli.serve_dir.as_deref().map(|dir| (dir, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use crate::dwarf_smoke::collect_debug_source_paths;
    use crate::paths_util::{
        canonical_display_path, detect_local_fastled_path, display_path, resolve_compile_directory,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    fn cwd_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct CurrentDirGuard {
        original: PathBuf,
    }

    impl CurrentDirGuard {
        fn enter(path: &Path) -> Self {
            let original = std::env::current_dir().expect("current dir");
            std::env::set_current_dir(path).expect("set current dir");
            Self { original }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    fn base_cli() -> Cli {
        Cli {
            directory: None,
            serve_dir: None,
            init: None,
            just_compile: false,
            no_app: false,
            link_mode: crate::cli::LinkMode::Static,
            profile: false,
            install: false,
            dry_run: false,
            no_interactive: false,
            no_https: false,
            latest: false,
            branch: None,
            commit: None,
            fastled_path: None,
            purge: false,
            clangd: false,
            write_clangd: None,
            internal_ensure_fastled_repo: None,
            internal_dwarf_smoke: false,
            internal_serve_dir_headless: None,
            debug: false,
            quick: false,
            release: false,
        }
    }

    #[test]
    fn serve_dir_request_headless_never_launches_viewer() {
        let mut cli = base_cli();
        cli.internal_serve_dir_headless = Some("some/dir".to_string());
        assert_eq!(serve_dir_request(&cli), Some(("some/dir", false)));
    }

    #[test]
    fn serve_dir_request_serve_dir_launches_viewer() {
        let mut cli = base_cli();
        cli.serve_dir = Some("some/dir".to_string());
        assert_eq!(serve_dir_request(&cli), Some(("some/dir", true)));
    }

    #[test]
    fn serve_dir_request_none_without_serve_flags() {
        assert_eq!(serve_dir_request(&base_cli()), None);
    }

    #[test]
    fn collect_debug_source_paths_finds_wasm_and_source_map_entries() {
        let temp = tempfile::tempdir().expect("tempdir");
        let output_dir = temp.path().join("fastled_js");
        fs::create_dir_all(&output_dir).unwrap();
        fs::write(
            output_dir.join("fastled.wasm"),
            b"\0sketchsource/Blink.ino\0prefix/fastledsource/FastLED.h\0",
        )
        .unwrap();
        fs::write(
            output_dir.join("fastled.wasm.map"),
            r#"{"version":3,"sources":["dwarfsource/emsdk/upstream/emscripten/cache.h","ignored.js"]}"#,
        )
        .unwrap();
        let config = debug_symbols::DebugSymbolConfig {
            sketch_dir: temp.path().join("Blink"),
            fastled_dir: Some(temp.path().join("FastLED")),
            emsdk_path: Some(temp.path().join("emsdk")),
            prefixes: debug_symbols::DwarfPrefixConfig::default(),
        };

        let paths = collect_debug_source_paths(&output_dir, &config).unwrap();

        assert!(paths.contains("sketchsource/Blink.ino"));
        assert!(paths.contains("prefix/fastledsource/FastLED.h"));
        assert!(paths.contains("dwarfsource/emsdk/upstream/emscripten/cache.h"));
        assert!(!paths.contains("ignored.js"));
    }

    #[test]
    fn resolve_compile_directory_uses_current_sketch_dir() {
        let _lock = cwd_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let sketch_dir = temp.path().join("Blink");
        fs::create_dir_all(&sketch_dir).unwrap();
        fs::write(sketch_dir.join("Blink.ino"), b"void setup() {}").unwrap();
        let _guard = CurrentDirGuard::enter(&sketch_dir);

        let resolved = resolve_compile_directory(&base_cli())
            .unwrap()
            .expect("sketch directory");
        assert_eq!(
            canonical_display_path(Path::new(&resolved)),
            canonical_display_path(&sketch_dir)
        );
    }

    #[test]
    fn resolve_compile_directory_accepts_file_inside_sketch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sketch_dir = temp.path().join("Blink");
        let source_file = sketch_dir.join("Blink.ino");
        fs::create_dir_all(&sketch_dir).unwrap();
        fs::write(&source_file, b"void setup() {}").unwrap();

        let mut cli = base_cli();
        cli.directory = Some(display_path(&source_file));

        let resolved = resolve_compile_directory(&cli).unwrap();
        assert_eq!(resolved, Some(canonical_display_path(&sketch_dir)));
    }

    #[test]
    fn resolve_compile_directory_matches_partial_name() {
        let _lock = cwd_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let examples_dir = temp.path().join("examples").join("FxWave2d");
        fs::create_dir_all(&examples_dir).unwrap();
        fs::write(examples_dir.join("FxWave2d.ino"), b"void setup() {}").unwrap();
        let _guard = CurrentDirGuard::enter(temp.path());

        let mut cli = base_cli();
        cli.directory = Some("FxWave2d".to_string());

        let resolved = resolve_compile_directory(&cli).unwrap();
        assert_eq!(
            resolved,
            Some(
                PathBuf::from("examples")
                    .join("FxWave2d")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }

    #[test]
    fn detect_local_fastled_path_finds_repo_upwards() {
        let _lock = cwd_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let repo_root = temp.path().join("FastLED");
        let nested = repo_root.join("examples").join("Blink");
        fs::create_dir_all(&nested).unwrap();
        fs::write(repo_root.join("library.properties"), b"name=FastLED\n").unwrap();
        let _guard = CurrentDirGuard::enter(&nested);

        let detected = detect_local_fastled_path();
        assert_eq!(detected, Some(canonical_display_path(&repo_root)));
    }

    #[test]
    fn resolve_prompt_choice_accepts_default_selection() {
        let options = vec!["Blink".to_string(), "Noise".to_string()];
        match resolve_prompt_choice("", &options, 1) {
            PromptChoice::Selected(choice) => assert_eq!(choice, "Noise"),
            _ => panic!("expected default selection"),
        }
    }

    #[test]
    fn resolve_prompt_choice_accepts_numeric_selection() {
        let options = vec!["Blink".to_string(), "Noise".to_string()];
        match resolve_prompt_choice("2", &options, 0) {
            PromptChoice::Selected(choice) => assert_eq!(choice, "Noise"),
            _ => panic!("expected numeric selection"),
        }
    }

    #[test]
    fn resolve_prompt_choice_narrows_partial_matches() {
        let options = vec![
            "Fire2012".to_string(),
            "Fire2012WithPalette".to_string(),
            "Blink".to_string(),
        ];
        match resolve_prompt_choice("Fire", &options, 0) {
            PromptChoice::Narrowed(matches) => {
                assert_eq!(
                    matches,
                    vec!["Fire2012".to_string(), "Fire2012WithPalette".to_string()]
                );
            }
            _ => panic!("expected narrowed partial matches"),
        }
    }

    #[test]
    fn resolve_prompt_choice_uses_fuzzy_matching() {
        let options = vec![
            "examples/Audio".to_string(),
            "examples/BeatDetection".to_string(),
            "examples/Blink".to_string(),
        ];
        match resolve_prompt_choice("beats", &options, 0) {
            PromptChoice::Selected(choice) => {
                assert_eq!(choice, "examples/BeatDetection");
            }
            _ => panic!("expected fuzzy selection"),
        }
    }

    #[test]
    fn prepare_sketch_selection_auto_selects_single_directory() {
        let result = prepare_sketch_selection(vec![PathBuf::from("sketch1")], false, false);
        assert_eq!(result, SketchSelection::Selected("sketch1".to_string()));
    }

    #[test]
    fn prepare_sketch_selection_defers_large_first_scan() {
        let sketches = (0..5)
            .map(|index| PathBuf::from(format!("sketch{index}")))
            .collect();
        let result = prepare_sketch_selection(sketches, false, false);
        assert_eq!(result, SketchSelection::None);
    }

    #[test]
    fn prepare_sketch_selection_prompts_large_followup_scan() {
        let sketches: Vec<PathBuf> = (0..5)
            .map(|index| PathBuf::from(format!("sketch{index}")))
            .collect();
        let result = prepare_sketch_selection(sketches, false, true);
        assert_eq!(
            result,
            SketchSelection::Prompt(
                (0..5)
                    .map(|index| format!("sketch{index}"))
                    .collect::<Vec<_>>()
            )
        );
    }

    #[test]
    fn prepare_sketch_selection_filters_fastled_repo_dirs() {
        let sketches = vec![
            PathBuf::from("src"),
            PathBuf::from("dev"),
            PathBuf::from("tests"),
            PathBuf::from("examples"),
        ];
        let result = prepare_sketch_selection(sketches, true, false);
        assert_eq!(result, SketchSelection::Selected("examples".to_string()));
    }
}
