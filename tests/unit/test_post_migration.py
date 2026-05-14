"""Tests for post-migration cleanup: dead code removal, stale exports, and bug fixes."""

import ast
import unittest
from pathlib import Path

ROOT_DIR = Path(__file__).parent.parent.parent
SRC_DIR = ROOT_DIR / "src" / "fastled"
FASTLED_CLI_SRC_DIR = ROOT_DIR / "crates" / "fastled-cli" / "src"


def _implementation_source(path: Path) -> str:
    lines = path.read_text().splitlines()
    return "\n".join(
        line for line in lines if not line.lstrip().startswith(("#", "//", "//!"))
    )


class TestPythonEntrypointsDelegateToRust(unittest.TestCase):
    """User-facing Python entry points should forward to the native Rust CLI."""

    def test_cli_main_uses_rust_launcher(self) -> None:
        source = (SRC_DIR / "cli.py").read_text()
        self.assertIn("invoke_rust_fastled_cli", source)

    def test_app_main_uses_rust_launcher(self) -> None:
        source = (SRC_DIR / "app.py").read_text()
        self.assertIn("invoke_rust_fastled_cli", source)

    def test_primary_runtime_has_no_fastled_app_module_fallback(self) -> None:
        runtime_files = [
            SRC_DIR / "app.py",
            SRC_DIR / "cli.py",
            SRC_DIR / "_rust_cli.py",
            SRC_DIR / "open_browser.py",
            *FASTLED_CLI_SRC_DIR.glob("*.rs"),
        ]

        fallback_patterns = [
            "python -m fastled.app",
            '"-m", "fastled.app"',
            "'-m', 'fastled.app'",
            '"-m",\n        "fastled.app"',
            "'-m',\n        'fastled.app'",
        ]

        for path in runtime_files:
            source = _implementation_source(path)
            for pattern in fallback_patterns:
                self.assertNotIn(
                    pattern,
                    source,
                    f"{path.relative_to(ROOT_DIR)} must not invoke the old Python app fallback",
                )

    def test_rust_cli_binary_uses_distinct_migration_name(self) -> None:
        cli_source = (SRC_DIR / "_rust_cli.py").read_text()
        cargo_manifest = ROOT_DIR / "crates" / "fastled-cli" / "Cargo.toml"
        self.assertIn("fastled-rs", cli_source)
        self.assertIn('name = "fastled-rs"', cargo_manifest.read_text())

    def test_python_fastled_shim_cannot_resolve_itself_on_path(self) -> None:
        cli_source = _implementation_source(SRC_DIR / "_rust_cli.py")
        self.assertNotIn('shutil.which("fastled")', cli_source)
        self.assertNotIn('"fastled.exe"', cli_source)
        self.assertIn('"fastled-rs.exe"', cli_source)
        self.assertIn("shutil.which(exe)", cli_source)
        self.assertIn("FASTLED_PYTHON_EXECUTABLE", cli_source)

    def test_build_rs_reexports_native_wasm_backend(self) -> None:
        build_source = (FASTLED_CLI_SRC_DIR / "build.rs").read_text()
        self.assertIn("pub use crate::wasm_build::{", build_source)
        self.assertIn("run_build", build_source)
        self.assertNotIn("configure_embedded_python_executable", build_source)


class TestNativeAppOrchestration(unittest.TestCase):
    """App-layer compatibility modules should prefer native Rust behavior."""

    COMPATIBILITY_MODULES = {
        "build_service.py": "Compatibility build-service facade backed by the native Rust service.",
        "build_types.py": "Python request/result DTOs for the native build service wrapper.",
        "frontend_esbuild.py": "Compatibility shim. Frontend bundling is implemented in Rust.",
        "open_browser.py": "Compatibility process launcher for the native Rust HTTP server.",
        "project_init.py": "Compatibility project-init helpers backed by native Rust operations.",
        "select_sketch_directory.py": "Compatibility prompt helpers backed by native Rust matching logic.",
        "sketch.py": "Sketch detection helpers.",
        "string_diff.py": "Compatibility wrappers for native string matching helpers.",
    }

    WRAPPER_MODULES_WITH_NATIVE_DELEGATES = [
        "build_service.py",
        "frontend_esbuild.py",
        "open_browser.py",
        "project_init.py",
        "select_sketch_directory.py",
        "sketch.py",
        "string_diff.py",
    ]

    EXPLICIT_PYTHON_HOLDOUTS = [
        "build_types.py",
        "select_sketch_directory.py",
    ]

    def test_select_sketch_directory_uses_native_resolver(self) -> None:
        source = (SRC_DIR / "select_sketch_directory.py").read_text()
        self.assertIn("_native_prepare_sketch_selection", source)
        self.assertIn("_native_resolve_prompt_choice", source)
        self.assertNotIn("from fastled.string_diff import string_diff", source)

    def test_build_service_has_no_python_fallback_service(self) -> None:
        source = (SRC_DIR / "build_service.py").read_text()
        self.assertIn("from fastled._native import NativeBuildService", source)
        self.assertNotIn("class _PythonBuildService", source)
        self.assertNotIn("if self._native is None", source)
        self.assertNotIn("python -m fastled.app", source)

    def test_remaining_python_modules_document_wrapper_or_holdout_role(self) -> None:
        for filename, expected_summary in self.COMPATIBILITY_MODULES.items():
            module = ast.parse((SRC_DIR / filename).read_text())
            docstring = ast.get_docstring(module)
            if docstring is None:
                self.fail(
                    f"{filename} should document its post-migration compatibility role"
                )
            self.assertEqual(
                docstring.splitlines()[0],
                expected_summary,
                f"{filename} should document its post-migration compatibility role",
            )

    def test_remaining_python_wrappers_delegate_to_native_helpers(self) -> None:
        for filename in self.WRAPPER_MODULES_WITH_NATIVE_DELEGATES:
            source = (SRC_DIR / filename).read_text()
            expected_delegate = (
                "fastled._rust_cli" if filename == "open_browser.py" else "fastled._native"
            )
            self.assertIn(
                expected_delegate,
                source,
                f"{filename} should delegate core behavior to native helpers",
            )

    def test_remaining_python_holdouts_are_not_fallback_service_owners(self) -> None:
        service_owner_markers = [
            "class _PythonBuildService",
            "class BuildService",
            "def build(",
            "NativeBuildService",
            "python -m fastled.app",
        ]

        for filename in self.EXPLICIT_PYTHON_HOLDOUTS:
            source = (SRC_DIR / filename).read_text()
            for marker in service_owner_markers:
                self.assertNotIn(
                    marker,
                    source,
                    f"{filename} is a compatibility holdout, not a fallback service owner",
                )

    def test_rust_build_orchestration_has_no_python_cli_fallback(self) -> None:
        source = (ROOT_DIR / "crates" / "fastled-cli" / "src" / "build.rs").read_text()
        self.assertNotIn("python -m " + "fastled.app", source)
        self.assertNotIn("run_build_" + "subprocess", source)

    def test_types_do_not_import_legacy_args_at_runtime(self) -> None:
        source = (SRC_DIR / "types.py").read_text()
        self.assertNotIn("from fastled.args import Args", source)


class TestInstallSiteWrapperCleanup(unittest.TestCase):
    """Install/site compatibility wrappers should not orchestrate fastled via PATH."""

    WRAPPER_PATHS = [
        SRC_DIR / "site" / "build.py",
        SRC_DIR / "install" / "examples_manager.py",
    ]

    def test_wrappers_do_not_use_shell_or_path_fastled_subprocesses(self) -> None:
        forbidden_patterns = [
            "shell=True",
            'which("fastled")',
            "which('fastled')",
            '["fastled",',
            "['fastled',",
        ]
        for path in self.WRAPPER_PATHS:
            source = _implementation_source(path)
            for pattern in forbidden_patterns:
                self.assertNotIn(
                    pattern,
                    source,
                    f"{path.relative_to(ROOT_DIR)} must delegate through native helpers",
                )

    def test_wrappers_delegate_to_native_cli_or_project_init_helpers(self) -> None:
        site_source = (SRC_DIR / "site" / "build.py").read_text()
        install_source = (SRC_DIR / "install" / "examples_manager.py").read_text()

        self.assertIn("project_init", site_source)
        self.assertIn("invoke_rust_fastled_cli", site_source)
        self.assertIn("invoke_rust_fastled_cli", install_source)


class TestNoDeadCodeModules(unittest.TestCase):
    """Bug 2: Dead code files left over from the backend cleanup must be removed."""

    DEAD_MODULES = [
        "find_good_connection",
        "interruptible_http",
        "zip_files",
        "header_dump",
        "toolchain/internal_wasm_build",
    ]

    def test_dead_modules_do_not_exist(self) -> None:
        for mod in self.DEAD_MODULES:
            path = SRC_DIR / f"{mod}.py"
            self.assertFalse(
                path.exists(),
                f"Dead code module {mod}.py should have been deleted",
            )

    def test_emscripten_wrapper_does_not_import_internal_builder(self) -> None:
        source = (SRC_DIR / "toolchain" / "emscripten.py").read_text()
        self.assertNotIn("internal_wasm_build", source)
        self.assertNotIn("subprocess.run", source)

    def test_legacy_native_toolchain_launchers_removed(self) -> None:
        native_tools = SRC_DIR / "toolchain" / "native_tools"
        self.assertFalse((native_tools / "launcher_emcc.cpp").exists())
        self.assertFalse((native_tools / "launcher_wasmld.cpp").exists())

    def test_playwright_package_removed(self) -> None:
        self.assertFalse((SRC_DIR / "playwright").exists())
        requirements = (ROOT_DIR / "requirements.testing.txt").read_text()
        self.assertNotIn("playwright", requirements.lower())

    def test_cli_removed_browser_mode_flags_and_extension_plumbing(self) -> None:
        cli_source = (FASTLED_CLI_SRC_DIR / "lib.rs").read_text()
        install_source = (FASTLED_CLI_SRC_DIR / "install.rs").read_text()
        vscode_source = (SRC_DIR / "install" / "vscode_config.py").read_text()

        for source in (cli_source, install_source, vscode_source):
            self.assertNotIn("--app", source)
            self.assertNotIn("--legacy-browser", source)
            self.assertNotIn("internal_ensure_chrome_extension", source)
            self.assertNotIn("chrome-extensions", source)
            self.assertNotIn("CHROME_CRX", source)

        self.assertIn("viewer::launch_tauri_viewer", cli_source)
        self.assertNotIn("ViewerMode::Browser", cli_source)
        self.assertNotIn("fn open_browser", cli_source)


class TestAllExportsValid(unittest.TestCase):
    """Bug 3: __all__ must not export stale symbols from the removed server code."""

    STALE_SYMBOLS = ["CompileServerError", "FileResponse"]

    def test_no_stale_exports(self) -> None:
        import fastled

        all_exports = getattr(fastled, "__all__", [])
        for symbol in self.STALE_SYMBOLS:
            self.assertNotIn(
                symbol,
                all_exports,
                f"Stale symbol '{symbol}' should not be in __all__",
            )


if __name__ == "__main__":
    unittest.main()
