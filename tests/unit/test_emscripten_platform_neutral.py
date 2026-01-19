# -*- coding: utf-8 -*-
"""
Unit tests for verifying the Emscripten toolchain is platform-neutral.

These tests verify that the emscripten.py module does not expose or require
platform/architecture information in its API, while still being able to
find and use Emscripten installations automatically.
"""

import inspect
import unittest


class TestEmscriptenPlatformNeutral(unittest.TestCase):
    """Test that the Emscripten toolchain API is platform-neutral."""

    def test_no_platform_arch_in_public_api(self) -> None:
        """Verify no public functions require platform or arch parameters."""
        from fastled.toolchain import emscripten

        # Get all public functions (no leading underscore)
        public_funcs = [
            name
            for name in dir(emscripten)
            if not name.startswith("_") and callable(getattr(emscripten, name))
        ]

        for func_name in public_funcs:
            func = getattr(emscripten, func_name)
            if not callable(func):
                continue

            # Skip classes - we're testing functions
            if isinstance(func, type):
                continue

            try:
                sig = inspect.signature(func)
            except (ValueError, TypeError):
                # Some builtins don't have signatures
                continue

            params = list(sig.parameters.keys())

            # Check that no public function requires platform or arch
            self.assertNotIn(
                "platform",
                params,
                f"Public function {func_name} should not require 'platform' parameter",
            )
            self.assertNotIn(
                "arch",
                params,
                f"Public function {func_name} should not require 'arch' parameter",
            )

    def test_no_get_platform_arch_exported(self) -> None:
        """Verify _get_platform_arch is not exported (should be private)."""
        from fastled.toolchain import emscripten

        # The function should either not exist or be private (start with _)
        public_names = [name for name in dir(emscripten) if not name.startswith("_")]

        self.assertNotIn(
            "get_platform_arch",
            public_names,
            "get_platform_arch should not be exported publicly",
        )

    def test_ensure_clang_tool_chain_emscripten_no_args(self) -> None:
        """Verify ensure_clang_tool_chain_emscripten takes no required arguments."""
        from fastled.toolchain.emscripten import ensure_clang_tool_chain_emscripten

        sig = inspect.signature(ensure_clang_tool_chain_emscripten)

        # Count required parameters (those without defaults)
        required_params = [
            name
            for name, param in sig.parameters.items()
            if param.default is inspect.Parameter.empty
            and param.kind not in (param.VAR_POSITIONAL, param.VAR_KEYWORD)
        ]

        self.assertEqual(
            len(required_params),
            0,
            f"ensure_clang_tool_chain_emscripten should have no required parameters, "
            f"but has: {required_params}",
        )

    def test_get_clang_tool_chain_emscripten_dir_no_args(self) -> None:
        """Verify _get_clang_tool_chain_emscripten_dir takes no required arguments."""
        from fastled.toolchain.emscripten import _get_clang_tool_chain_emscripten_dir

        sig = inspect.signature(_get_clang_tool_chain_emscripten_dir)

        # Count required parameters (those without defaults)
        required_params = [
            name
            for name, param in sig.parameters.items()
            if param.default is inspect.Parameter.empty
            and param.kind not in (param.VAR_POSITIONAL, param.VAR_KEYWORD)
        ]

        self.assertEqual(
            len(required_params),
            0,
            f"_get_clang_tool_chain_emscripten_dir should have no required parameters, "
            f"but has: {required_params}",
        )

    def test_emscripten_toolchain_init_no_platform_args(self) -> None:
        """Verify EmscriptenToolchain.__init__ doesn't require platform/arch."""
        from fastled.toolchain.emscripten import EmscriptenToolchain

        sig = inspect.signature(EmscriptenToolchain.__init__)
        params = list(sig.parameters.keys())

        # Remove 'self' from params
        params = [p for p in params if p != "self"]

        self.assertNotIn(
            "platform", params, "EmscriptenToolchain should not require 'platform'"
        )
        self.assertNotIn(
            "arch", params, "EmscriptenToolchain should not require 'arch'"
        )

    def test_can_instantiate_toolchain_without_args(self) -> None:
        """Verify EmscriptenToolchain can be instantiated without arguments."""
        from fastled.toolchain.emscripten import EmscriptenToolchain

        # This should not raise any exceptions
        toolchain = EmscriptenToolchain()
        self.assertIsNotNone(toolchain)

    def test_docstrings_no_platform_arch_paths(self) -> None:
        """Verify docstrings don't mention platform/arch in paths."""
        from fastled.toolchain import emscripten

        # Check module docstring
        module_doc = emscripten.__doc__ or ""
        self.assertNotIn(
            "{platform}",
            module_doc,
            "Module docstring should not reference {platform} in paths",
        )
        self.assertNotIn(
            "{arch}",
            module_doc,
            "Module docstring should not reference {arch} in paths",
        )

        # Check function docstrings
        for name in dir(emscripten):
            obj = getattr(emscripten, name)
            if callable(obj) and hasattr(obj, "__doc__") and obj.__doc__:
                self.assertNotIn(
                    "{platform}",
                    obj.__doc__,
                    f"{name} docstring should not reference {{platform}} in paths",
                )
                self.assertNotIn(
                    "{arch}",
                    obj.__doc__,
                    f"{name} docstring should not reference {{arch}} in paths",
                )

    def test_emscripten_dir_found_without_platform_arg(self) -> None:
        """Verify the clang-tool-chain dir function finds installation without platform args."""
        from fastled.toolchain.emscripten import _get_clang_tool_chain_emscripten_dir

        # This function should find the Emscripten installation automatically
        # without requiring any platform/architecture arguments
        result = _get_clang_tool_chain_emscripten_dir()

        # The function should return a path or None (depending on installation)
        # but it should NOT require any platform/arch parameters to do so
        if result is not None:
            # If found, verify it contains the expected emscripten scripts
            emscripten_scripts = result / "emscripten"
            self.assertTrue(
                emscripten_scripts.exists(),
                f"Expected emscripten scripts directory at: {emscripten_scripts}",
            )
            emcc_py = emscripten_scripts / "emcc.py"
            self.assertTrue(
                emcc_py.exists(),
                f"Expected emcc.py at: {emcc_py}",
            )


class TestEmscriptenNoImportOfPlatformModule(unittest.TestCase):
    """Test that platform module import was removed."""

    def test_no_platform_module_used_for_arch(self) -> None:
        """Verify the platform module is not used to detect architecture."""
        import ast
        from pathlib import Path

        # Read the source file
        emscripten_path = (
            Path(__file__).parent.parent.parent
            / "src"
            / "fastled"
            / "toolchain"
            / "emscripten.py"
        )

        source = emscripten_path.read_text()
        tree = ast.parse(source)

        # Look for imports of 'platform' module
        platform_imports = []
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                for alias in node.names:
                    if alias.name == "platform":
                        platform_imports.append(node.lineno)
            elif isinstance(node, ast.ImportFrom):
                if node.module == "platform":
                    platform_imports.append(node.lineno)

        self.assertEqual(
            len(platform_imports),
            0,
            f"'platform' module should not be imported, but found imports at lines: {platform_imports}",
        )


if __name__ == "__main__":
    unittest.main()
