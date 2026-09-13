use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

#[derive(Clone, Debug)]
pub(crate) enum Command {
    /// Inspect and explicitly manage the installed Emscripten toolchain.
    Toolchain { action: ToolchainAction },
    /// Inspect and explicitly manage cached FastLED source checkouts.
    Source { action: SourceAction },
}

#[derive(Clone, Debug)]
pub(crate) enum ToolchainAction {
    /// Show the active package and local installation state.
    Status,
    /// Install the current release default without activating it.
    Install {
        /// Install a specific catalog package ID.
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

/// Explicit management actions for the cached FastLED source checkout.
#[derive(Clone, Debug)]
pub(crate) enum SourceAction {
    /// Show the cached source revision, fetch time, and freshness state.
    Status {
        /// Cached FastLED ref to inspect.
        reference: String,
    },
    /// Download a fresh source checkout while preserving the old checkout on failure.
    Update {
        /// Cached FastLED ref to refresh.
        reference: String,
    },
    /// Remove one cached FastLED source checkout.
    Purge {
        /// Cached FastLED ref to remove.
        reference: String,
    },
}

/// Parse the standalone management trees through kernal-api. The primary
/// build grammar remains on the staged migration path below.
pub(crate) fn management_command_from<I, S>(arguments: I) -> Result<Option<Command>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    use kernal_api::command::{Command as SchemaCommand, OptionSpec, ValueKind};

    let arguments = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_os_string())
        .collect::<Vec<OsString>>();
    if arguments
        .iter()
        .skip(1)
        .any(|argument| matches!(argument.to_str(), Some("--help" | "-h")))
    {
        return Ok(None);
    }
    // Keep the root build options on this tree so options preceding a
    // management subcommand retain the same acceptance as the former Clap
    // grammar (for example, `--no-interactive source status`).
    let schema = primary_schema()
        .subcommand(
            SchemaCommand::new("toolchain")
                .subcommand(SchemaCommand::new("status"))
                .subcommand(
                    SchemaCommand::new("install")
                        .option(OptionSpec::value("package-id", ValueKind::string())),
                )
                .subcommand(
                    SchemaCommand::new("activate")
                        .positional("activate-package-id", ValueKind::string()),
                )
                .subcommand(SchemaCommand::new("update"))
                .subcommand(
                    SchemaCommand::new("repair")
                        .optional_positional("repair-package-id", ValueKind::string()),
                )
                .subcommand(SchemaCommand::new("rollback"))
                .subcommand(SchemaCommand::new("prune")),
        )
        .subcommand(
            SchemaCommand::new("source")
                .option(OptionSpec::value("ref", ValueKind::string()).default("master"))
                .subcommand(SchemaCommand::new("status"))
                .subcommand(SchemaCommand::new("update"))
                .subcommand(SchemaCommand::new("purge")),
        );
    let parsed = schema.parse(arguments).map_err(|error| error.to_string())?;
    validate_ref_options(
        parsed.flag("latest").unwrap_or(false),
        parsed.value("branch").is_some(),
        parsed.value("commit").is_some(),
    )?;
    let path = parsed.command_path();
    let command = match path {
        [_, tree, action] if tree == "toolchain" => Command::Toolchain {
            action: match action.as_str() {
                "status" => ToolchainAction::Status,
                "install" => ToolchainAction::Install {
                    package_id: parsed.value("package-id").map(str::to_owned),
                },
                "activate" => ToolchainAction::Activate {
                    package_id: parsed
                        .value("activate-package-id")
                        .unwrap_or_default()
                        .to_owned(),
                },
                "update" => ToolchainAction::Update,
                "repair" => ToolchainAction::Repair {
                    package_id: parsed.value("repair-package-id").map(str::to_owned),
                },
                "rollback" => ToolchainAction::Rollback,
                "prune" => ToolchainAction::Prune,
                _ => return Err("invalid toolchain command".to_owned()),
            },
        },
        [_, tree, action] if tree == "source" => Command::Source {
            action: match action.as_str() {
                "status" => SourceAction::Status {
                    reference: parsed.value("ref").unwrap_or_default().to_owned(),
                },
                "update" => SourceAction::Update {
                    reference: parsed.value("ref").unwrap_or_default().to_owned(),
                },
                "purge" => SourceAction::Purge {
                    reference: parsed.value("ref").unwrap_or_default().to_owned(),
                },
                _ => return Err("invalid source command".to_owned()),
            },
        },
        [_, tree] if matches!(tree.as_str(), "toolchain" | "source") => {
            return Err("invalid management command".to_owned());
        }
        _ => return Ok(None),
    };
    Ok(Some(command))
}

pub(crate) fn primary_schema() -> kernal_api::command::Command {
    use kernal_api::command::{Command as SchemaCommand, OptionSpec, ValueKind};

    SchemaCommand::new("fastled")
        .about("FastLED WASM compilation CLI")
        .version(env!("CARGO_PKG_VERSION"))
        .optional_positional("directory", ValueKind::string())
        .option(OptionSpec::value("serve-dir", ValueKind::string()).help("Serve an existing directory without compiling."))
        .option(OptionSpec::value("init", ValueKind::string()).optional_value("__init__").help("Initialize a sketch; without a value uses the default example."))
        .option(OptionSpec::flag("just-compile").help("Compile without opening the viewer."))
        .option(OptionSpec::flag("no-app").help("Emit the JavaScript API and WASM artifacts only."))
        .option(
            OptionSpec::value("link", ValueKind::enumeration(["static", "dynamic"]))
                .default("static").help("Select static (default) or dynamic linking."),
        )
        .option(OptionSpec::flag("profile").help("Enable C++ build profiling."))
        .option(OptionSpec::flag("install").help("Install the FastLED development environment."))
        .option(OptionSpec::flag("dry-run").help("Simulate actions without changing state."))
        .option(OptionSpec::flag("no-interactive").help("Fail instead of prompting."))
        .option(OptionSpec::flag("no-https").help("Use HTTP for the local server."))
        .option(
            OptionSpec::flag("test")
                .conflicts("just-compile")
                .conflicts("no-app").help("Compile, render, collect artifacts, and exit."),
        )
        .option(
            OptionSpec::flag("check")
                .conflicts("just-compile")
                .conflicts("no-app").help("Render one frame and fail on browser-side errors."),
        )
        .exclusive_group("production-test", ["test", "check"])
        .option(
            OptionSpec::value("test-wait-secs", ValueKind::f64())
                .default("1")
                .requires_any(["test", "check"]).help("Seconds to wait before the first capture (default: 1)."),
        )
        .option(
            OptionSpec::value("test-screenshot", ValueKind::os_string())
                .requires_any(["test", "check"])
                .help("PNG output path. Interval mode supports an unpadded index, {n:03}, and {ts}."),
        )
        .option(
            OptionSpec::value("test-interval-secs", ValueKind::f64())
                .requires_any(["test", "check"])
                .help("Target interval between scheduled capture starts; slow captures may delay later frames."),
        )
        .option(
            OptionSpec::value("test-count", ValueKind::u32())
                .requires_any(["test", "check"])
                .conflicts("test-duration-secs").help("Number of screenshots in interval mode."),
        )
        .option(
            OptionSpec::value("test-duration-secs", ValueKind::f64())
                .requires_any(["test", "check"])
                .conflicts("test-count").help("Total capture window in seconds."),
        )
        .option(
            OptionSpec::value("test-log", ValueKind::os_string()).requires_any(["test", "check"]).help("Append viewer output to this file."),
        )
        .option(OptionSpec::flag("test-exit-on-error").requires_any(["test", "check"]).help("Exit with code 2 on a viewer error."))
        .option(
            OptionSpec::value("test-timeout-secs", ValueKind::f64())
                .default("120")
                .requires_any(["test", "check"]).help("Hard timeout in seconds (default: 120)."),
        )
        .option(
            OptionSpec::value("test-ready-timeout-secs", ValueKind::f64())
                .default("15")
                .requires_any(["test", "check"]).help("Canvas-ready timeout in seconds (default: 15)."),
        )
        .option(
            OptionSpec::value("test-cmd", ValueKind::string())
                .repeated()
                .requires_any(["test", "check"]).help("Trusted command after the first frame; may repeat."),
        )
        .option(OptionSpec::flag("latest").help("Use the latest tagged FastLED release for initialization."))
        .option(OptionSpec::value("branch", ValueKind::string()).help("FastLED branch for initialization."))
        .option(OptionSpec::value("commit", ValueKind::string()).help("FastLED commit for initialization."))
        .option(OptionSpec::value("fastled-path", ValueKind::string()).help("Path to the FastLED library."))
        .option(OptionSpec::flag("purge").help("Purge the cached FastLED checkout."))
        .option(OptionSpec::flag("clangd").help("Emit VS Code clangd configuration after compiling."))
        .option(OptionSpec::value("write-clangd", ValueKind::string()).optional_value("__cwd__").help("Write clangd configuration and exit."))
        .option(OptionSpec::flag("write-intellisense-snapshot").hidden())
        .option(
            OptionSpec::value("internal-ensure-fastled-repo", ValueKind::string())
                .optional_value("__latest__")
                .hidden(),
        )
        .option(OptionSpec::flag("internal-dwarf-smoke").hidden())
        .option(OptionSpec::value("internal-serve-dir-headless", ValueKind::string()).hidden())
        .option(
            OptionSpec::flag("debug")
                .conflicts("quick")
                .conflicts("release"),
        )
        .option(
            OptionSpec::flag("quick")
                .conflicts("debug")
                .conflicts("release"),
        )
        .option(
            OptionSpec::flag("release")
                .conflicts("debug")
                .conflicts("quick"),
        )
}

/// Render facade-owned presentation text without reintroducing a parser dependency.
pub(crate) fn presentation_help(arguments: &[OsString]) -> String {
    use kernal_api::command::{Command as SchemaCommand, OptionSpec, ValueKind};

    let words = arguments
        .iter()
        .skip(1)
        .take_while(|argument| argument.as_os_str() != "--")
        .filter_map(|argument| argument.to_str())
        .collect::<Vec<_>>();
    let value_options = [
        "--serve-dir",
        "--init",
        "--link",
        "--test-wait-secs",
        "--test-screenshot",
        "--test-interval-secs",
        "--test-count",
        "--test-duration-secs",
        "--test-log",
        "--test-timeout-secs",
        "--test-ready-timeout-secs",
        "--test-cmd",
        "--branch",
        "--commit",
        "--fastled-path",
        "--write-clangd",
        "--internal-ensure-fastled-repo",
        "--internal-serve-dir-headless",
    ];
    let mut index = 0;
    let management_index = loop {
        let Some(word) = words.get(index) else {
            break None;
        };
        if matches!(*word, "toolchain" | "source") {
            break Some(index);
        }
        index += if value_options.contains(word) && !word.contains('=') {
            2
        } else {
            1
        };
    };
    let Some(index) = management_index else {
        return format!("{}\nCommands:\n  toolchain\tManage Emscripten toolchains.\n  source\tManage cached FastLED sources.\n", primary_schema().render_help());
    };
    let command = match &words[index..] {
        ["toolchain", "install", ..] => SchemaCommand::new("fastled toolchain install").option(
            OptionSpec::value("package-id", ValueKind::string())
                .help("Catalog package ID to install."),
        ),
        ["toolchain", "activate", ..] => SchemaCommand::new("fastled toolchain activate")
            .positional("package-id", ValueKind::string()),
        ["toolchain", "repair", ..] => SchemaCommand::new("fastled toolchain repair")
            .optional_positional("package-id", ValueKind::string()),
        ["toolchain", action @ ("status" | "update" | "rollback" | "prune"), ..] => {
            SchemaCommand::new(format!("fastled toolchain {action}"))
        }
        ["toolchain", ..] => SchemaCommand::new("fastled toolchain")
            .subcommand(SchemaCommand::new("status"))
            .subcommand(SchemaCommand::new("install"))
            .subcommand(SchemaCommand::new("activate"))
            .subcommand(SchemaCommand::new("update"))
            .subcommand(SchemaCommand::new("repair"))
            .subcommand(SchemaCommand::new("rollback"))
            .subcommand(SchemaCommand::new("prune")),
        ["source", action @ ("status" | "update" | "purge"), ..] => {
            SchemaCommand::new(format!("fastled source {action}")).option(
                OptionSpec::value("ref", ValueKind::string())
                    .default("master")
                    .help("FastLED ref to inspect."),
            )
        }
        ["source", ..] => SchemaCommand::new("fastled source")
            .subcommand(SchemaCommand::new("status"))
            .subcommand(SchemaCommand::new("update"))
            .subcommand(SchemaCommand::new("purge")),
        _ => SchemaCommand::new("fastled"),
    };
    command.render_help()
}

pub(crate) fn primary_cli_from<I, S>(arguments: I) -> Result<Cli, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let parsed = primary_schema()
        .parse(arguments)
        .map_err(|error| error.to_string())?;
    let flag = |name| parsed.flag(name).unwrap_or(false);
    let string = |name| parsed.value(name).map(str::to_owned);
    let link_mode = match parsed.value("link") {
        Some("dynamic") => LinkMode::Dynamic,
        Some("static") | None => LinkMode::Static,
        Some(_) => return Err("invalid link mode".to_owned()),
    };
    Ok(Cli {
        command: None,
        directory: string("directory"),
        serve_dir: string("serve-dir"),
        init: string("init"),
        just_compile: flag("just-compile"),
        no_app: flag("no-app"),
        link_mode,
        profile: flag("profile"),
        install: flag("install"),
        dry_run: flag("dry-run"),
        no_interactive: flag("no-interactive"),
        no_https: flag("no-https"),
        test: flag("test"),
        check: flag("check"),
        test_wait_secs: parsed.f64("test-wait-secs").unwrap_or(1.0),
        test_screenshot: parsed.os_value("test-screenshot").map(PathBuf::from),
        test_interval_secs: parsed.f64("test-interval-secs"),
        test_count: parsed.u32("test-count"),
        test_duration_secs: parsed.f64("test-duration-secs"),
        test_log: parsed.os_value("test-log").map(PathBuf::from),
        test_exit_on_error: flag("test-exit-on-error"),
        test_timeout_secs: parsed.f64("test-timeout-secs").unwrap_or(120.0),
        test_ready_timeout_secs: parsed.f64("test-ready-timeout-secs").unwrap_or(15.0),
        test_cmd: parsed.values("test-cmd").unwrap_or_default().to_vec(),
        latest: flag("latest"),
        branch: string("branch"),
        commit: string("commit"),
        fastled_path: string("fastled-path"),
        purge: flag("purge"),
        clangd: flag("clangd"),
        write_clangd: string("write-clangd"),
        write_intellisense_snapshot: flag("write-intellisense-snapshot"),
        internal_ensure_fastled_repo: string("internal-ensure-fastled-repo"),
        internal_dwarf_smoke: flag("internal-dwarf-smoke"),
        internal_serve_dir_headless: string("internal-serve-dir-headless"),
        debug: flag("debug"),
        quick: flag("quick"),
        release: flag("release"),
    })
}

/// How the sketch code is linked into the generated WASM program.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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
#[derive(Debug)]
pub(crate) struct Cli {
    pub(crate) command: Option<Command>,

    /// Directory containing the FastLED sketch to compile.
    pub(crate) directory: Option<String>,

    /// Serve an existing directory without compiling a sketch.
    pub(crate) serve_dir: Option<String>,

    /// Initialize a FastLED sketch in the current directory.
    /// An optional example name may be provided (e.g. --init Blink).
    pub(crate) init: Option<String>,

    /// Just compile; skip opening the browser and watching for changes.
    pub(crate) just_compile: bool,

    /// Omit the default index.js application and emit only the JavaScript API
    /// plus its WASM/runtime artifacts.
    pub(crate) no_app: bool,

    /// Select static linking (the default) or Emscripten side-module linking.
    pub(crate) link_mode: LinkMode,

    /// Enable profiling of the C++ build system used for WASM compilation.
    pub(crate) profile: bool,

    /// Install the FastLED development environment with VSCode configuration.
    pub(crate) install: bool,

    /// Run in dry-run mode (simulate actions without making changes).
    pub(crate) dry_run: bool,

    /// Run in non-interactive mode (fail instead of prompting for input).
    pub(crate) no_interactive: bool,

    /// Disable HTTPS and use HTTP for the local server.
    #[allow(dead_code)] // Parsed compatibility flag; server policy has not adopted it yet.
    pub(crate) no_https: bool,

    /// Compile, render, collect requested test artifacts, and exit.
    pub(crate) test: bool,

    /// Compile and render one frame, failing on browser-side errors.
    pub(crate) check: bool,

    /// Seconds to wait after the sketch is ready before the first capture.
    pub(crate) test_wait_secs: f64,

    /// PNG output path. Interval mode supports an unpadded index, {n:03}, and {ts}.
    pub(crate) test_screenshot: Option<PathBuf>,

    /// Target interval between scheduled capture starts; slow captures may delay later frames.
    pub(crate) test_interval_secs: Option<f64>,

    /// Number of screenshots to take in interval mode.
    pub(crate) test_count: Option<u32>,

    /// Total capture window in seconds for interval mode.
    pub(crate) test_duration_secs: Option<f64>,

    /// Append viewer console and error output to this file.
    pub(crate) test_log: Option<PathBuf>,

    /// Exit with code 2 when the viewer reports a page-side error.
    pub(crate) test_exit_on_error: bool,

    /// Hard upper bound in seconds for compile, ready wait, and capture.
    pub(crate) test_timeout_secs: f64,

    /// Seconds to wait for a rendered canvas after compilation succeeds.
    pub(crate) test_ready_timeout_secs: f64,

    /// Run a trusted host command after the first rendered frame. May be repeated.
    pub(crate) test_cmd: Vec<String>,

    /// Use the latest tagged FastLED release when initialising examples with --init.
    /// Defaults to `master`; tagged releases older than the meson migration cannot be built.
    pub(crate) latest: bool,

    /// Use a specific branch when initialising examples with --init.
    pub(crate) branch: Option<String>,

    /// Use a specific commit SHA when initialising examples with --init.
    pub(crate) commit: Option<String>,

    /// Path to the FastLED library for native compilation.
    pub(crate) fastled_path: Option<String>,

    /// Purge the cached FastLED repo, forcing a fresh re-download on next build.
    pub(crate) purge: bool,

    /// Also emit VS Code clangd configuration (compile_commands.json,
    /// .clangd, .vscode/settings.json) into the sketch directory after a
    /// successful compile.
    pub(crate) clangd: bool,

    /// Write VS Code clangd configuration (compile_commands.json,
    /// .clangd, .vscode/settings.json) for the sketch directory and exit.
    /// Defaults to the current directory when no DIR is given.
    pub(crate) write_clangd: Option<String>,

    /// Read a versioned JSON document snapshot from stdin and atomically
    /// refresh the ignored IntelliSense cache. Used by the VS Code extension.
    pub(crate) write_intellisense_snapshot: bool,

    /// Internal plumbing flag: ensure the FastLED repo for the given ref
    /// (defaults to latest release) is downloaded and extracted, print the
    /// local path to stdout, and exit. Used by the Python `Api.project_init`
    /// path so the Python side never has to do an HTTP download.
    pub(crate) internal_ensure_fastled_repo: Option<String>,

    /// Internal CI plumbing: compile a debug sketch, start the source server
    /// without the viewer, and verify every embedded debug source path.
    pub(crate) internal_dwarf_smoke: bool,

    /// Internal CI plumbing: serve compiled output without launching the viewer.
    pub(crate) internal_serve_dir_headless: Option<String>,

    // Build mode (mutually exclusive).
    /// Build in debug mode.
    pub(crate) debug: bool,

    /// Build in quick mode (default).
    #[allow(dead_code)]
    // Parsed compatibility flag; quick is selected by the absence of a mode.
    pub(crate) quick: bool,

    /// Build in optimised release mode.
    pub(crate) release: bool,
}

#[cfg(test)]
impl Cli {
    pub(crate) fn parse_from<I, S>(arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Self::try_parse_from(arguments).expect("test command-line arguments must be valid")
    }

    pub(crate) fn try_parse_from<I, S>(arguments: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let arguments = arguments
            .into_iter()
            .map(|argument| argument.as_ref().to_os_string())
            .collect::<Vec<_>>();
        if let Some(command) = management_command_from(&arguments)? {
            let mut cli = primary_cli_from(["fastled"])?;
            cli.command = Some(command);
            return Ok(cli);
        }
        primary_cli_from(arguments)
    }
}

pub(crate) fn validate_init_ref_flags(cli: &Cli) -> Result<(), &'static str> {
    validate_ref_options(cli.latest, cli.branch.is_some(), cli.commit.is_some())
}

fn validate_ref_options(
    latest: bool,
    has_branch: bool,
    has_commit: bool,
) -> Result<(), &'static str> {
    if latest && (has_branch || has_commit) {
        return Err("--latest cannot be used with --branch or --commit");
    }
    Ok(())
}

pub(crate) fn apply_test_implications(cli: &mut Cli) {
    if cli.check {
        cli.test = true;
        cli.test_exit_on_error = true;
    }
    if cli.test {
        cli.no_interactive = true;
    }
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
    fn management_commands_use_the_kernel_schema() {
        assert!(matches!(
            management_command_from(["fastled", "toolchain", "install", "--package-id", "package-a"]),
            Ok(Some(Command::Toolchain {
                action: ToolchainAction::Install { package_id: Some(package_id) }
            })) if package_id == "package-a"
        ));
        assert!(matches!(
            management_command_from(["fastled", "source", "update", "--ref", "main"]),
            Ok(Some(Command::Source {
                action: SourceAction::Update { reference }
            })) if reference == "main"
        ));
        assert!(matches!(
            management_command_from(["fastled", "--no-interactive", "source", "status"]),
            Ok(Some(Command::Source {
                action: SourceAction::Status { reference }
            })) if reference == "master"
        ));
        assert!(matches!(
            management_command_from(["fastled", "source", "--help"]),
            Ok(None)
        ));
        assert!(matches!(
            management_command_from(["fastled", "source"]),
            Err(message) if message == "invalid management command"
        ));
        assert!(matches!(
            management_command_from(["fastled", "--latest", "--branch", "main", "source", "status"]),
            Err(message) if message == "--latest cannot be used with --branch or --commit"
        ));
        assert!(matches!(
            management_command_from(["fastled", "source", "update", "--unknown"]),
            Err(message) if message == "invalid command-line arguments"
        ));
    }

    #[test]
    fn primary_schema_captures_build_test_and_hidden_flags() {
        let parsed = primary_schema()
            .parse([
                "fastled",
                "sketch",
                "--link=dynamic",
                "--test",
                "--test-count=2",
                "--test-cmd=echo one",
                "--test-cmd",
                "echo two",
                "--write-intellisense-snapshot",
            ])
            .unwrap();
        assert_eq!(parsed.value("directory"), Some("sketch"));
        assert_eq!(parsed.value("link"), Some("dynamic"));
        assert_eq!(parsed.u32("test-count"), Some(2));
        assert_eq!(parsed.values("test-cmd").unwrap().len(), 2);
        assert!(parsed.flag("write-intellisense-snapshot").unwrap());
        assert!(!primary_schema()
            .render_help()
            .contains("write-intellisense-snapshot"));
    }

    #[test]
    fn primary_cli_mapping_preserves_typed_defaults_and_paths() {
        let cli = primary_cli_from([
            "fastled",
            "sketch",
            "--test",
            "--test-count=2",
            "--test-screenshot=out.png",
            "--link=dynamic",
        ])
        .unwrap();
        assert_eq!(cli.directory.as_deref(), Some("sketch"));
        assert_eq!(cli.test_count, Some(2));
        assert_eq!(cli.test_screenshot, Some(PathBuf::from("out.png")));
        assert_eq!(cli.test_timeout_secs, 120.0);
        assert_eq!(cli.link_mode, LinkMode::Dynamic);
        let defaults = primary_cli_from(["fastled", "sketch"]);
        assert!(defaults.is_ok(), "default controls must not require --test");
        assert!(primary_cli_from(["fastled", "--", "--help"]).is_ok());
    }

    #[test]
    fn clangd_emission_is_opt_in() {
        let cli = Cli::parse_from(["fastled", "sketch"]);
        assert!(!cli.clangd);
        let cli = Cli::parse_from(["fastled", "--clangd", "sketch"]);
        assert!(cli.clangd);
    }

    #[test]
    fn parses_stdin_intellisense_snapshot_command() {
        let cli = Cli::parse_from(["fastled", "--write-intellisense-snapshot"]);
        assert!(cli.write_intellisense_snapshot);
    }

    #[test]
    fn api_and_dynamic_linking_flags_parse_together() {
        let cli = Cli::parse_from(["fastled", "sketch", "--no-app", "--link=dynamic"]);
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

    #[test]
    fn source_commands_are_explicit_subcommands() {
        let cli = Cli::parse_from(["fastled", "source", "status"]);
        assert!(matches!(
            cli.command,
            Some(Command::Source {
                action: SourceAction::Status { reference }
            }) if reference == "master"
        ));

        let cli = Cli::parse_from(["fastled", "source", "update", "--ref", "main"]);
        assert!(matches!(
            cli.command,
            Some(Command::Source {
                action: SourceAction::Update { reference }
            }) if reference == "main"
        ));

        let cli = Cli::parse_from(["fastled", "source", "purge", "--ref", "3.10.0"]);
        assert!(matches!(
            cli.command,
            Some(Command::Source {
                action: SourceAction::Purge { reference }
            }) if reference == "3.10.0"
        ));
    }

    #[test]
    fn production_test_flags_parse_as_a_group() {
        let cli = Cli::parse_from([
            "fastled",
            "sketch",
            "--test",
            "--test-wait-secs=2",
            "--test-interval-secs=0.5",
            "--test-count=10",
            "--test-screenshot=out-{n:03}.png",
            "--test-log=run.log",
            "--test-exit-on-error",
        ]);
        assert!(cli.test);
        assert_eq!(cli.test_wait_secs, 2.0);
        assert_eq!(cli.test_interval_secs, Some(0.5));
        assert_eq!(cli.test_count, Some(10));
        assert_eq!(cli.test_screenshot, Some(PathBuf::from("out-{n:03}.png")));
        assert_eq!(cli.test_log, Some(PathBuf::from("run.log")));
        assert!(cli.test_exit_on_error);
        assert!(cli.test_cmd.is_empty());
    }

    #[test]
    fn repeated_test_commands_preserve_declaration_order() {
        let cli = Cli::parse_from([
            "fastled",
            "sketch",
            "--test",
            "--test-cmd=first",
            "--test-cmd",
            "second",
        ]);
        assert_eq!(cli.test_cmd, vec!["first", "second"]);
    }

    #[test]
    fn test_commands_require_master_switch() {
        assert!(Cli::try_parse_from(["fastled", "sketch", "--test-cmd=echo hi"]).is_err());
    }

    #[test]
    fn production_test_help_describes_filename_templates() {
        let help = primary_schema().render_help();
        assert!(help.contains("supports an unpadded index, {n:03}, and {ts}"));
        assert!(help.contains("Target interval between scheduled capture starts"));
    }

    #[test]
    fn test_mode_implies_non_interactive_before_directory_resolution() {
        let mut cli = Cli::parse_from(["fastled", "--test"]);
        assert!(!cli.no_interactive);
        apply_test_implications(&mut cli);
        assert!(cli.no_interactive);
    }

    #[test]
    fn check_is_a_strict_one_frame_test() {
        let mut cli = Cli::parse_from(["fastled", "sketch", "--check", "--test-timeout-secs=30"]);
        assert!(!cli.test);
        assert!(!cli.test_exit_on_error);
        apply_test_implications(&mut cli);
        assert!(cli.test);
        assert!(cli.test_exit_on_error);
        assert!(cli.no_interactive);
        assert!(cli.test_screenshot.is_none());
        assert_eq!(cli.test_timeout_secs, 30.0);
    }

    #[test]
    fn production_test_suboptions_require_master_switch() {
        assert!(Cli::try_parse_from([
            "fastled",
            "sketch",
            "--test-count=2",
            "--test-interval-secs=.5",
            "--test-screenshot=out-{n}.png"
        ])
        .is_err());
    }

    #[test]
    fn production_test_count_and_duration_conflict() {
        assert!(Cli::try_parse_from([
            "fastled",
            "sketch",
            "--test",
            "--test-count=2",
            "--test-duration-secs=1",
            "--test-interval-secs=.5",
            "--test-screenshot=out-{n}.png"
        ])
        .is_err());
    }
}
