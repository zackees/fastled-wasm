"""Tests for post-migration cleanup: dead code removal, stale exports, and bug fixes."""

import ast
import inspect
import unittest
from pathlib import Path

SRC_DIR = Path(__file__).parent.parent.parent / "src" / "fastled"


class TestAppFlagPassedToNativeCompile(unittest.TestCase):
    """Bug 1: --app flag must be accepted and forwarded by run_native_compile."""

    def test_run_native_compile_accepts_app_param(self) -> None:
        from fastled.compile_native import run_native_compile

        sig = inspect.signature(run_native_compile)
        self.assertIn(
            "app",
            sig.parameters,
            "run_native_compile must accept an 'app' parameter",
        )

    def test_app_main_passes_app_to_run_native_compile(self) -> None:
        """Verify app.py passes args.app in the run_native_compile() call."""
        source = (SRC_DIR / "app.py").read_text()
        self.assertIn(
            "app=",
            source,
            "app.py should pass app= keyword to run_native_compile",
        )


class TestNoDeadCodeModules(unittest.TestCase):
    """Bug 2: Dead code files left over from Docker/server migration must be removed."""

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
