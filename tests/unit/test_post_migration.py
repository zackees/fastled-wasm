"""Tests for post-migration cleanup: dead code removal, stale exports, and bug fixes."""

import ast
import unittest
from pathlib import Path

SRC_DIR = Path(__file__).parent.parent.parent / "src" / "fastled"


class TestPythonEntrypointsDelegateToRust(unittest.TestCase):
    """User-facing Python entry points should forward to the native Rust CLI."""

    def test_cli_main_uses_rust_launcher(self) -> None:
        source = (SRC_DIR / "cli.py").read_text()
        self.assertIn("invoke_rust_fastled_cli", source)

    def test_app_main_uses_rust_launcher(self) -> None:
        source = (SRC_DIR / "app.py").read_text()
        self.assertIn("invoke_rust_fastled_cli", source)


class TestNativeAppOrchestration(unittest.TestCase):
    """App-layer compatibility modules should prefer native Rust behavior."""

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

    def test_types_do_not_import_legacy_args_at_runtime(self) -> None:
        source = (SRC_DIR / "types.py").read_text()
        self.assertNotIn("from fastled.args import Args", source)


class TestNoDeadCodeModules(unittest.TestCase):
    """Bug 2: Dead code files left over from the backend cleanup must be removed."""

    DEAD_MODULES = [
        "find_good_connection",
        "interruptible_http",
        "zip_files",
        "header_dump",
    ]

    def test_dead_modules_do_not_exist(self) -> None:
        for mod in self.DEAD_MODULES:
            path = SRC_DIR / f"{mod}.py"
            self.assertFalse(
                path.exists(),
                f"Dead code module {mod}.py should have been deleted",
            )


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


class TestNoRedundantNativeField(unittest.TestCase):
    """Bug 5: The always-True 'native' field should be removed from Args."""

    def test_args_has_no_native_field(self) -> None:
        from fastled.args import Args

        fields = {f.name for f in Args.__dataclass_fields__.values()}
        self.assertNotIn(
            "native",
            fields,
            "Args.native is always True and should be removed",
        )

    def test_parse_args_no_native_flag(self) -> None:
        """The --native argparse flag should be removed."""
        source = (SRC_DIR / "parse_args.py").read_text()
        tree = ast.parse(source)
        for node in ast.walk(tree):
            if isinstance(node, ast.Constant) and node.value == "--native":
                self.fail("parse_args.py should not define a --native argparse flag")


if __name__ == "__main__":
    unittest.main()
