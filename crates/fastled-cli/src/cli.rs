use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum Command {
    /// Inspect and explicitly manage the installed Emscripten toolchain.
    Toolchain {
        #[command(subcommand)]
        action: ToolchainAction,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum ToolchainAction {
    /// Show the active package and local installation state.
    Status,
    /// Install the current release default without activating it.
    Install {
        /// Install a specific catalog package ID.
        #[arg(long)]
        package_id: Option<String>,
    },
    /// Health-check and activate an installed catalog package.
    Activate {
        /// Catalog package ID to activate.
        package_id: String,
    },
    /// Install, health-check, and activate the current release default.
    Update,
    /// Reinstall and activate the current release default explicitly.
    Repair {
        /// Catalog package ID to repair, defaulting to the release default.
        package_id: Option<String>,
    },
    /// Reactivate the previous known-good package without network access.
    Rollback,
    /// Remove inactive package installations.
    Prune,
}

/// How the sketch code is linked into the generated WASM program.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum LinkMode {
    /// Statically link the sketch into fastled.wasm.
    #[default]
    Static,
    /// Emit sketch.wasm as an Emscripten side module loaded by fastled.js.
    Dynamic,
}

/// FastLED WASM compilation CLI.
///
/// Rust front-end for FastLED WASM workflows.
///
/// Native Rust owns the full user-facing CLI surface, including compile
/// orchestration through `build.rs`.
#[derive(Parser, Debug)]
#[command(
    name = "fastled",
    version,
    about = "FastLED WASM compilation CLI",
    long_about = None,
    // Stop Rust clap from eating flags it doesn't recognise; we want to be
    // a strict mirror, so every flag is declared explicitly.
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,

    /// Directory containing the FastLED sketch to compile.
    pub(crate) directory: Option<String>,

    /// Serve an existing directory without compiling a sketch.
    #[arg(long, value_name = "DIR")]
    pub(crate) serve_dir: Option<String>,

    /// Initialize a FastLED sketch in the current directory.
    /// An optional example name may be provided (e.g. --init Blink).
    #[arg(long, value_name = "EXAMPLE", num_args = 0..=1, default_missing_value = "__init__")]
    pub(crate) init: Option<String>,

    /// Just compile; skip opening the browser and watching for changes.
    #[arg(long)]
    pub(crate) just_compile: bool,

    /// Omit the default index.js application and emit only the JavaScript API
    /// plus its WASM/runtime artifacts.
    #[arg(long)]
    pub(crate) no_app: bool,

    /// Select static linking (the default) or Emscripten side-module linking.
    #[arg(long, value_enum, default_value_t = LinkMode::Static)]
    pub(crate) link_mode: LinkMode,

    /// Enable profiling of the C++ build system used for WASM compilation.
    #[arg(long)]
    pub(crate) profile: bool,

    /// Install the FastLED development environment with VSCode configuration.
    #[arg(long)]
    pub(crate) install: bool,

    /// Run in dry-run mode (simulate actions without making changes).
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Run in non-interactive mode (fail instead of prompting for input).
    #[arg(long)]
    pub(crate) no_interactive: bool,

    /// Disable HTTPS and use HTTP for the local server.
    #[arg(long)]
    pub(crate) no_https: bool,

    /// Use the latest tagged FastLED release when initialising examples with --init.
    /// Defaults to `master`; tagged releases older than the meson migration cannot be built.
    #[arg(long)]
    pub(crate) latest: bool,

    /// Use a specific branch when initialising examples with --init.
    #[arg(long, value_name = "BRANCH")]
    pub(crate) branch: Option<String>,

    /// Use a specific commit SHA when initialising examples with --init.
    #[arg(long, value_name = "SHA")]
    pub(crate) commit: Option<String>,

    /// Path to the FastLED library for native compilation.
    #[arg(long, value_name = "PATH")]
    pub(crate) fastled_path: Option<String>,

    /// Purge the cached FastLED repo, forcing a fresh re-download on next build.
    #[arg(long)]
    pub(crate) purge: bool,

    /// Also emit VS Code clangd configuration (compile_commands.json,
    /// .clangd, .vscode/settings.json) into the sketch directory after a
    /// successful compile.
    #[arg(long)]
    pub(crate) clangd: bool,

    /// Write VS Code clangd configuration (compile_commands.json,
    /// .clangd, .vscode/settings.json) for the sketch directory and exit.
    /// Defaults to the current directory when no DIR is given.
    #[arg(long, value_name = "DIR", num_args = 0..=1, default_missing_value = "__cwd__")]
    pub(crate) write_clangd: Option<String>,

    /// Internal plumbing flag: ensure the FastLED repo for the given ref
    /// (defaults to latest release) is downloaded and extracted, print the
    /// local path to stdout, and exit. Used by the Python `Api.project_init`
    /// path so the Python side never has to do an HTTP download.
    #[arg(long, value_name = "REF", num_args = 0..=1, default_missing_value = "__latest__", hide = true)]
    pub(crate) internal_ensure_fastled_repo: Option<String>,

    /// Internal CI plumbing: compile a debug sketch, start the source server
    /// without the viewer, and verify every embedded debug source path.
    #[arg(long, hide = true)]
    pub(crate) internal_dwarf_smoke: bool,

    /// Internal CI plumbing: serve compiled output without launching the viewer.
    #[arg(long, value_name = "DIR", hide = true)]
    pub(crate) internal_serve_dir_headless: Option<String>,

    // Build mode (mutually exclusive).
    /// Build in debug mode.
    #[arg(long, conflicts_with_all = ["quick", "release"])]
    pub(crate) debug: bool,

    /// Build in quick mode (default).
    #[arg(long, conflicts_with_all = ["debug", "release"])]
    pub(crate) quick: bool,

    /// Build in optimised release mode.
    #[arg(long, conflicts_with_all = ["debug", "quick"])]
    pub(crate) release: bool,
}

pub(crate) fn validate_init_ref_flags(cli: &Cli) -> Result<(), &'static str> {
    if cli.latest && (cli.branch.is_some() || cli.commit.is_some()) {
        return Err("--latest cannot be used with --branch or --commit");
    }
    Ok(())
}

pub(crate) fn requested_init_ref(cli: &Cli) -> Option<&str> {
    if cli.latest {
        // `--latest` opts into the most recent tagged FastLED release. The
        // build path only supports refs that include `meson.build`, so users
        // who pass `--latest` are responsible for a release new enough to
        // ship the meson backend.
        None
    } else if let Some(explicit) = cli.commit.as_deref().or(cli.branch.as_deref()) {
        Some(explicit)
    } else {
        // Default to `master`. Tagged releases before the meson migration
        // (≤ 3.10.x) do not contain `meson.build`, so the unconditional
        // "latest release" default produced sketches that the build path
        // could not compile.
        Some("master")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clangd_emission_is_opt_in() {
        let cli = Cli::parse_from(["fastled", "sketch"]);
        assert!(!cli.clangd);
        let cli = Cli::parse_from(["fastled", "--clangd", "sketch"]);
        assert!(cli.clangd);
    }

    #[test]
    fn api_and_dynamic_linking_flags_parse_together() {
        let cli = Cli::parse_from(["fastled", "sketch", "--no-app", "--link-mode=dynamic"]);
        assert!(cli.no_app);
        assert_eq!(cli.link_mode, LinkMode::Dynamic);
    }

    #[test]
    fn static_linking_is_the_default() {
        let cli = Cli::parse_from(["fastled", "sketch"]);
        assert!(!cli.no_app);
        assert_eq!(cli.link_mode, LinkMode::Static);
    }

    #[test]
    fn toolchain_status_is_a_subcommand() {
        let cli = Cli::parse_from(["fastled", "toolchain", "status"]);
        assert!(matches!(
            cli.command,
            Some(Command::Toolchain {
                action: ToolchainAction::Status
            })
        ));
    }
}
