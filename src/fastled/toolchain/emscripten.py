"""
Emscripten Toolchain for FastLED

Provides native compilation using the Emscripten SDK (EMSDK) for compiling
FastLED sketches to WebAssembly without Docker.

This toolchain leverages FastLED's built-in WASM platform support located
in src/platforms/wasm, which provides Arduino-compatible stubs and WASM
bindings.

The toolchain supports two modes:
1. clang-tool-chain package (preferred): Uses the clang-tool-chain pip package
   which auto-downloads and manages Emscripten. Installation directory:
   ~/.clang-tool-chain/emscripten/
2. Standard EMSDK: Falls back to system EMSDK if clang-tool-chain is not available.
"""

import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

from fastled.types import BuildMode


def _get_clang_tool_chain_emscripten_dir() -> Path | None:
    """
    Get the clang-tool-chain Emscripten installation directory.

    The clang-tool-chain package installs Emscripten to:
    ~/.clang-tool-chain/emscripten/

    This function searches for the actual installation location which contains
    the 'emscripten' subdirectory with emcc.py and other scripts.

    Returns:
        Path to Emscripten installation directory, or None if not found.
    """
    # Check for environment variable override
    env_path = os.environ.get("CLANG_TOOL_CHAIN_DOWNLOAD_PATH")
    if env_path:
        base_dir = Path(env_path)
    else:
        base_dir = Path.home() / ".clang-tool-chain"

    emscripten_base = base_dir / "emscripten"

    if not emscripten_base.exists():
        return None

    # Check if this is the actual installation (has emscripten/emcc.py)
    if (emscripten_base / "emscripten" / "emcc.py").exists():
        return emscripten_base

    # Search for the installation in subdirectories (e.g., win/x86_64/)
    # This handles legacy platform-specific installations
    for subdir in emscripten_base.iterdir():
        if subdir.is_dir():
            # Check one level deep (e.g., emscripten/win/)
            for arch_dir in subdir.iterdir():
                if arch_dir.is_dir():
                    if (arch_dir / "emscripten" / "emcc.py").exists():
                        return arch_dir
            # Also check directly (e.g., emscripten/darwin/)
            if (subdir / "emscripten" / "emcc.py").exists():
                return subdir

    return None


def ensure_clang_tool_chain_emscripten() -> Path | None:
    """
    Ensure clang-tool-chain Emscripten is installed.

    If clang-tool-chain is installed as a pip package, this will trigger
    the auto-download of Emscripten on first use.

    Returns:
        Path to Emscripten installation directory, or None if clang-tool-chain
        is not available.
    """
    # First check if already installed
    existing_dir = _get_clang_tool_chain_emscripten_dir()
    if existing_dir:
        return existing_dir

    # Try to import clang-tool-chain and trigger installation
    try:
        # Try platform-neutral API first (preferred)
        import inspect

        from clang_tool_chain.installers.emscripten import (  # type: ignore[import-not-found]
            ensure_emscripten_available,
            get_emscripten_install_dir,
        )

        sig = inspect.signature(ensure_emscripten_available)
        if len(sig.parameters) == 0:
            # New platform-neutral API
            ensure_emscripten_available()
            return get_emscripten_install_dir()
        else:
            # Caller handles this case - we don't have platform/arch info
            # Just return None and let the fallback mechanisms handle it
            return None
    except ImportError:
        # clang-tool-chain package not installed
        return None
    except Exception as e:
        print(f"Warning: Failed to ensure clang-tool-chain Emscripten: {e}")
        return None


@dataclass
class CompilerPaths:
    """Paths to Emscripten compiler executables."""

    emcc: Path
    empp: Path  # em++
    emar: Path


@dataclass
class EmscriptenConfig:
    """Configuration for Emscripten compilation."""

    build_mode: BuildMode
    output_name: str = "fastled"
    # Common flags for all builds
    common_flags: tuple[str, ...] = (
        "-s",
        "WASM=1",
        "-s",
        "MODULARIZE=1",
        "-s",
        "EXPORT_ES6=1",
        "-s",
        "EXPORTED_RUNTIME_METHODS=['ccall','cwrap']",
        "-s",
        "EXPORTED_FUNCTIONS=['_main','_extern_setup','_extern_loop']",
        "-s",
        "ALLOW_MEMORY_GROWTH=1",
        "-s",
        "INITIAL_MEMORY=16777216",  # 16MB
        "-s",
        "STACK_SIZE=1048576",  # 1MB stack
        "-DFASTLED_WASM",
        "-DFASTLED_USE_STUB_ARDUINO",  # Enable stub timing declarations (millis, micros, etc.)
        "-DEMSCRIPTEN_HAS_UNBOUND_TYPE_NAMES=0",  # Required when using -fno-rtti with emscripten/val.h
        "-std=c++17",
        "-fno-rtti",
        "-fno-exceptions",
        "-Wno-error=undefined",  # Downgrade undefined symbol errors to warnings
    )

    def get_optimization_flags(self) -> list[str]:
        """Get optimization flags based on build mode."""
        if self.build_mode == BuildMode.DEBUG:
            return ["-g", "-O0", "-s", "ASSERTIONS=2"]
        elif self.build_mode == BuildMode.QUICK:
            return ["-O1", "-s", "ASSERTIONS=1"]
        else:  # RELEASE
            return ["-O3", "-flto", "-s", "ASSERTIONS=0"]


class EmscriptenToolchain:
    """
    Emscripten toolchain for compiling FastLED sketches to WebAssembly.

    This toolchain compiles Arduino/FastLED sketches directly using the
    Emscripten SDK without requiring Docker. It uses FastLED's built-in
    WASM platform support.
    """

    FASTLED_GITHUB_URL = "https://github.com/FastLED/FastLED"
    FASTLED_ARCHIVE_URL = (
        "https://github.com/FastLED/FastLED/archive/refs/heads/master.zip"
    )

    def __init__(
        self,
        fastled_path: Path | str | None = None,
        emsdk_path: Path | str | None = None,
    ):
        """
        Initialize the Emscripten toolchain.

        Args:
            fastled_path: Path to FastLED library. If None, downloads from master.
            emsdk_path: Path to EMSDK installation. If None, searches PATH.
        """
        self._fastled_path: Path | None = Path(fastled_path) if fastled_path else None
        self._emsdk_path: Path | None = Path(emsdk_path) if emsdk_path else None
        self._compiler_paths: CompilerPaths | None = None
        self._temp_dir: Path | None = None

    def _find_emsdk(self) -> Path | None:
        """Find EMSDK installation path.

        Priority order:
        1. clang-tool-chain package installation (~/.clang-tool-chain/emscripten/)
        2. EMSDK environment variable
        3. Common installation paths (~/emsdk, /opt/emsdk, C:/emsdk, etc.)
        """
        # Priority 1: Check clang-tool-chain installation
        clang_tool_chain_dir = _get_clang_tool_chain_emscripten_dir()
        if clang_tool_chain_dir and clang_tool_chain_dir.exists():
            # clang-tool-chain structure:
            # ~/.clang-tool-chain/emscripten/
            #   ├── bin/           - Contains clang, clang++, wasm-ld, wasm-opt, etc.
            #   ├── emscripten/    - Contains emcc.py, em++.py, emar.py, etc.
            #   └── .emscripten    - Config file
            if (clang_tool_chain_dir / "emscripten").exists():
                return clang_tool_chain_dir

        # Priority 2: Check EMSDK environment variable
        emsdk_env = os.environ.get("EMSDK")
        if emsdk_env:
            return Path(emsdk_env)

        # Priority 3: Check common installation paths
        common_paths = [
            Path.home() / "emsdk",
            Path("/opt/emsdk"),
            Path("C:/emsdk"),
            Path.home() / ".emsdk",
            Path.home() / "AppData" / "Local" / "emsdk",
        ]

        for path in common_paths:
            if path.exists() and (path / "upstream" / "emscripten").exists():
                return path

        return None

    def _find_compilers(self) -> CompilerPaths:
        """Find Emscripten compiler executables.

        Supports two installation types:
        1. clang-tool-chain: Emscripten scripts are Python files in emscripten/ subdirectory
        2. Standard EMSDK: Emscripten scripts are in upstream/emscripten/
        """
        # First try to ensure clang-tool-chain Emscripten is available
        # This will auto-download if clang-tool-chain package is installed
        clang_tool_chain_dir = ensure_clang_tool_chain_emscripten()
        if clang_tool_chain_dir and clang_tool_chain_dir.exists():
            # clang-tool-chain structure:
            # ~/.clang-tool-chain/emscripten/
            #   ├── bin/           - Contains clang, clang++, wasm-ld, wasm-opt, etc.
            #   ├── emscripten/    - Contains emcc.py, em++.py, emar.py, etc.
            #   └── .emscripten    - Config file
            emscripten_scripts_dir = clang_tool_chain_dir / "emscripten"
            if emscripten_scripts_dir.exists():
                # Emscripten tools in clang-tool-chain are Python scripts (.py files)
                emcc_path = emscripten_scripts_dir / "emcc.py"
                empp_path = emscripten_scripts_dir / "em++.py"
                emar_path = emscripten_scripts_dir / "emar.py"

                if emcc_path.exists() and empp_path.exists():
                    return CompilerPaths(
                        emcc=emcc_path,
                        empp=empp_path,
                        emar=emar_path if emar_path.exists() else emcc_path,
                    )

        # Try clang-tool-chain wrapper commands if available in PATH
        ctc_emcc = shutil.which("clang-tool-chain-emcc")
        ctc_empp = shutil.which("clang-tool-chain-em++")
        ctc_emar = shutil.which("clang-tool-chain-emar")

        if ctc_emcc and ctc_empp:
            return CompilerPaths(
                emcc=Path(ctc_emcc),
                empp=Path(ctc_empp),
                emar=Path(ctc_emar) if ctc_emar else Path(ctc_emcc),
            )

        # Try standard Emscripten in PATH
        emcc = shutil.which("emcc")
        empp = shutil.which("em++")
        emar = shutil.which("emar")

        if emcc and empp and emar:
            return CompilerPaths(
                emcc=Path(emcc),
                empp=Path(empp),
                emar=Path(emar),
            )

        # Try EMSDK path
        emsdk = self._emsdk_path or self._find_emsdk()
        if emsdk:
            # Check if this is a clang-tool-chain directory
            if (emsdk / "emscripten").exists() and not (emsdk / "upstream").exists():
                # clang-tool-chain structure
                emscripten_scripts_dir = emsdk / "emscripten"
                emcc_path = emscripten_scripts_dir / "emcc.py"
                empp_path = emscripten_scripts_dir / "em++.py"
                emar_path = emscripten_scripts_dir / "emar.py"
            else:
                # Standard EMSDK structure
                emscripten_dir = emsdk / "upstream" / "emscripten"
                if sys.platform == "win32":
                    emcc_path = emscripten_dir / "emcc.bat"
                    empp_path = emscripten_dir / "em++.bat"
                    emar_path = emscripten_dir / "emar.bat"
                else:
                    emcc_path = emscripten_dir / "emcc"
                    empp_path = emscripten_dir / "em++"
                    emar_path = emscripten_dir / "emar"

            if emcc_path.exists() and empp_path.exists():
                return CompilerPaths(
                    emcc=emcc_path,
                    empp=empp_path,
                    emar=emar_path if emar_path.exists() else emcc_path,
                )

        raise FileNotFoundError(
            "Emscripten SDK not found. Please install using one of these methods:\n\n"
            "Option 1 (Recommended): Install clang-tool-chain package:\n"
            "  pip install clang-tool-chain\n"
            "  clang-tool-chain-emcc --version  # Auto-downloads Emscripten on first use\n\n"
            "Option 2: Install EMSDK manually:\n"
            "  https://emscripten.org/docs/getting_started/downloads.html\n"
            "  Or set EMSDK environment variable to point to your EMSDK installation."
        )

    def _download_fastled(self, target_dir: Path) -> Path:
        """Download FastLED library from GitHub master branch."""
        import io
        import zipfile

        import httpx

        print("Downloading FastLED library from GitHub master...")

        response = httpx.get(
            self.FASTLED_ARCHIVE_URL, follow_redirects=True, timeout=60
        )
        response.raise_for_status()

        # Extract the zip
        with zipfile.ZipFile(io.BytesIO(response.content)) as zf:
            zf.extractall(target_dir)

        # The extracted folder is named FastLED-master
        fastled_dir = target_dir / "FastLED-master"
        if not fastled_dir.exists():
            # Try to find any extracted directory
            dirs = [
                d
                for d in target_dir.iterdir()
                if d.is_dir() and d.name.startswith("FastLED")
            ]
            if dirs:
                fastled_dir = dirs[0]
            else:
                raise FileNotFoundError("Failed to extract FastLED library")

        return fastled_dir

    def _get_fastled_path(self) -> Path:
        """Get the path to FastLED library, downloading if necessary."""
        if self._fastled_path and self._fastled_path.exists():
            return self._fastled_path

        # Create a temp directory for downloaded FastLED
        if self._temp_dir is None:
            self._temp_dir = Path(tempfile.mkdtemp(prefix="fastled_native_"))

        # Download FastLED
        fastled_dir = self._download_fastled(self._temp_dir)
        self._fastled_path = fastled_dir
        return fastled_dir

    def _get_wasm_sources(self, fastled_dir: Path) -> list[Path]:
        """Get FastLED WASM platform source files."""
        src_dir = fastled_dir / "src"
        wasm_dir = src_dir / "platforms" / "wasm"
        stub_dir = src_dir / "platforms" / "stub"
        shared_dir = src_dir / "platforms" / "shared"

        sources: list[Path] = []

        # Add WASM platform sources
        if wasm_dir.exists():
            sources.extend(wasm_dir.rglob("*.cpp"))

        # Add stub platform sources (used by WASM)
        if stub_dir.exists():
            sources.extend(stub_dir.rglob("*.cpp"))

        # Add shared platform sources (contains ActiveStripData, UI, etc.)
        if shared_dir.exists():
            sources.extend(shared_dir.rglob("*.cpp"))

        return sources

    def _get_fastled_core_sources(self, fastled_dir: Path) -> list[Path]:
        """Get FastLED core source files (excluding platform-specific)."""
        src_dir = fastled_dir / "src"
        if not src_dir.exists():
            raise FileNotFoundError(f"FastLED src directory not found at {src_dir}")

        sources: list[Path] = []
        for pattern in ["*.cpp", "*.c"]:
            sources.extend(src_dir.glob(pattern))  # Only top-level src files

        # Add fl/ subdirectory sources
        fl_dir = src_dir / "fl"
        if fl_dir.exists():
            for pattern in ["*.cpp", "*.c"]:
                sources.extend(fl_dir.rglob(pattern))

        # Add fx/ subdirectory sources
        fx_dir = src_dir / "fx"
        if fx_dir.exists():
            for pattern in ["*.cpp", "*.c"]:
                sources.extend(fx_dir.rglob(pattern))

        return sources

    def _get_sketch_sources(self, sketch_dir: Path) -> list[Path]:
        """Get sketch source files (.ino, .cpp, .c)."""
        sources: list[Path] = []

        # Find .ino files and treat them as .cpp
        for ino_file in sketch_dir.glob("*.ino"):
            sources.append(ino_file)

        # Find .cpp and .c files
        for pattern in ["*.cpp", "*.c"]:
            sources.extend(sketch_dir.glob(pattern))

        return sources

    def _create_sketch_wrapper(
        self, sketch_sources: list[Path], output_dir: Path
    ) -> Path:
        """Create sketch.cpp wrapper that provides setup() and loop() for FastLED WASM platform.

        FastLED's WASM platform (entry_point.cpp) already provides:
        - main() function
        - extern_setup() / extern_loop() exports

        We just need to provide the user's setup() and loop() functions.
        """
        # Read sketch content
        sketch_content = ""
        for src in sketch_sources:
            if src.suffix == ".ino":
                sketch_content += f"\n// From {src.name}\n"
                sketch_content += src.read_text()

        wrapper_content = f"""
// Auto-generated sketch wrapper for FastLED WASM native compilation
// FastLED's entry_point.cpp provides main(), extern_setup(), extern_loop()
// This file provides the user's setup() and loop() functions

#include "FastLED.h"

// Include sketch code which defines setup() and loop()
{sketch_content}
"""
        sketch_path = output_dir / "sketch.cpp"
        sketch_path.write_text(wrapper_content)
        return sketch_path

    def compile(
        self,
        sketch_dir: Path,
        output_dir: Path,
        build_mode: BuildMode = BuildMode.QUICK,
        profile: bool = False,
    ) -> Path:
        """
        Compile a FastLED sketch to WebAssembly.

        Args:
            sketch_dir: Path to the sketch directory
            output_dir: Output directory for compiled files
            build_mode: Build mode (DEBUG, QUICK, RELEASE)
            profile: Enable profiling output

        Returns:
            Path to the compiled JavaScript file
        """
        if self._compiler_paths is None:
            self._compiler_paths = self._find_compilers()

        config = EmscriptenConfig(build_mode=build_mode)

        # Get FastLED library
        fastled_dir = self._get_fastled_path()
        print(f"Using FastLED library at: {fastled_dir}")

        # Create output directory
        output_dir.mkdir(parents=True, exist_ok=True)

        # Create build directory
        build_dir = output_dir / ".build"
        build_dir.mkdir(exist_ok=True)

        # Get source files
        sketch_sources = self._get_sketch_sources(sketch_dir)

        if not sketch_sources:
            raise FileNotFoundError(f"No sketch files found in {sketch_dir}")

        # Create sketch wrapper (FastLED provides main/entry points)
        sketch_file = self._create_sketch_wrapper(sketch_sources, build_dir)

        # Build include paths
        include_paths = [
            f"-I{fastled_dir}/src",
            f"-I{fastled_dir}/src/platforms/wasm",
            f"-I{fastled_dir}/src/platforms/wasm/compiler",
            f"-I{fastled_dir}/src/platforms/stub",
            f"-I{fastled_dir}/src/platforms/shared",
            f"-I{sketch_dir}",
        ]

        # Build compiler command
        output_file = output_dir / f"{config.output_name}.js"

        # Get and add source files
        core_sources = self._get_fastled_core_sources(fastled_dir)
        wasm_sources = self._get_wasm_sources(fastled_dir)
        all_sources = core_sources + wasm_sources

        print(
            f"Compiling with {len(sketch_sources)} sketch files and {len(all_sources)} FastLED files..."
        )

        # Use a response file to avoid Windows command line length limits
        # The response file contains all arguments, one per line
        # Convert Windows backslashes to forward slashes for Emscripten compatibility
        response_file = build_dir / "compile_args.rsp"

        def to_posix_path(p: str) -> str:
            """Convert path to forward slashes for Emscripten."""
            return p.replace("\\", "/")

        response_content_lines = [
            *[to_posix_path(p) for p in include_paths],
            *config.common_flags,
            *config.get_optimization_flags(),
            "-o",
            to_posix_path(str(output_file)),
            to_posix_path(str(sketch_file)),
            *[to_posix_path(str(s)) for s in all_sources],
        ]
        response_content = "\n".join(response_content_lines)
        response_file.write_text(response_content)

        # Build command based on compiler type
        empp_path = self._compiler_paths.empp
        env = os.environ.copy()

        # Check if this is a clang-tool-chain installation (Python script)
        if empp_path.suffix == ".py":
            # clang-tool-chain: Run the .py script with Python
            # Need to set up environment variables for Emscripten
            clang_tool_chain_dir = _get_clang_tool_chain_emscripten_dir()
            if clang_tool_chain_dir:
                # Set up Emscripten environment variables
                env["EMSCRIPTEN"] = str(clang_tool_chain_dir / "emscripten")
                env["EMSCRIPTEN_ROOT"] = str(clang_tool_chain_dir / "emscripten")
                config_path = clang_tool_chain_dir / ".emscripten"
                if config_path.exists():
                    env["EM_CONFIG"] = str(config_path)
                # Add bin directory to PATH for clang, wasm-opt, etc.
                bin_dir = clang_tool_chain_dir / "bin"
                if bin_dir.exists():
                    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"

            cmd = [
                sys.executable,  # Python interpreter
                str(empp_path),
                f"@{response_file}",
            ]
        elif "clang-tool-chain" in str(empp_path):
            # clang-tool-chain wrapper script (clang-tool-chain-em++)
            # These handle their own environment setup
            cmd = [
                str(empp_path),
                f"@{response_file}",
            ]
        else:
            # Standard EMSDK: Run emcc/em++ directly
            cmd = [
                str(empp_path),
                f"@{response_file}",
            ]

        if profile:
            print(f"Response file: {response_file}")
            print(f"Compile command: {' '.join(cmd)}")
            if empp_path.suffix == ".py":
                print("Using clang-tool-chain Emscripten")
                print(f"EMSCRIPTEN={env.get('EMSCRIPTEN', 'not set')}")
                print(f"EM_CONFIG={env.get('EM_CONFIG', 'not set')}")

        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                check=True,
                env=env,
            )
            if result.stdout:
                print(result.stdout)
        except subprocess.CalledProcessError as e:
            error_msg = f"Compilation failed:\n{e.stderr}\n{e.stdout}"
            print(error_msg)
            raise RuntimeError(error_msg) from e

        # Copy full production frontend assets from FastLED
        print("Copying frontend assets...")
        self._copy_frontend_assets(output_dir, fastled_dir)

        # Cleanup build directory (intermediate compilation files)
        if build_dir.exists():
            shutil.rmtree(build_dir, ignore_errors=True)

        # Cleanup temp directory (downloaded FastLED)
        if self._temp_dir and self._temp_dir.exists():
            shutil.rmtree(self._temp_dir, ignore_errors=True)
            self._temp_dir = None

        wasm_file = output_dir / f"{config.output_name}.wasm"
        print("Compilation successful!")
        print(f"  JS:   {output_file}")
        print(f"  WASM: {wasm_file}")

        return output_file

    def _copy_frontend_assets(self, output_dir: Path, fastled_dir: Path) -> None:
        """Copy the full production frontend assets from FastLED's wasm compiler directory.

        This copies the production-ready frontend files including:
        - index.html - Full-featured UI with menus, controls
        - index.css - Styling
        - index.js - Application logic
        - modules/ - JavaScript modules for audio, graphics, UI, recording

        These assets provide feature parity with the Docker build output.
        """
        compiler_assets_dir = fastled_dir / "src" / "platforms" / "wasm" / "compiler"

        if not compiler_assets_dir.exists():
            print(
                f"Warning: Frontend assets not found at {compiler_assets_dir}, using minimal index.html"
            )
            self._create_minimal_index_html(output_dir, "fastled")
            return

        # Copy index.html
        index_html = compiler_assets_dir / "index.html"
        if index_html.exists():
            shutil.copy2(index_html, output_dir / "index.html")
            print(f"  Copied: index.html ({index_html.stat().st_size} bytes)")

        # Copy index.css
        index_css = compiler_assets_dir / "index.css"
        if index_css.exists():
            shutil.copy2(index_css, output_dir / "index.css")
            print(f"  Copied: index.css ({index_css.stat().st_size} bytes)")

        # Copy index.js
        index_js = compiler_assets_dir / "index.js"
        if index_js.exists():
            shutil.copy2(index_js, output_dir / "index.js")
            print(f"  Copied: index.js ({index_js.stat().st_size} bytes)")

        # Copy modules directory
        modules_dir = compiler_assets_dir / "modules"
        if modules_dir.exists():
            output_modules_dir = output_dir / "modules"
            output_modules_dir.mkdir(parents=True, exist_ok=True)
            shutil.copytree(
                modules_dir,
                output_modules_dir,
                dirs_exist_ok=True,
                ignore=shutil.ignore_patterns(".*"),  # Ignore hidden files
            )
            module_count = len(list(output_modules_dir.rglob("*.js")))
            print(f"  Copied: modules/ ({module_count} JavaScript modules)")

        # Copy vendor directory (contains Three.js for 3D rendering)
        vendor_dir = compiler_assets_dir / "vendor"
        if vendor_dir.exists():
            output_vendor_dir = output_dir / "vendor"
            output_vendor_dir.mkdir(parents=True, exist_ok=True)
            shutil.copytree(
                vendor_dir,
                output_vendor_dir,
                dirs_exist_ok=True,
                ignore=shutil.ignore_patterns(".*"),  # Ignore hidden files
            )
            vendor_count = len(list(output_vendor_dir.rglob("*.js")))
            print(f"  Copied: vendor/ ({vendor_count} vendor files)")

        # Create empty files.json manifest (for consistency with Docker build)
        files_json = output_dir / "files.json"
        files_json.write_text("[]")
        print("  Created: files.json (empty manifest)")

    def _create_minimal_index_html(self, output_dir: Path, module_name: str) -> None:
        """Create a minimal fallback index.html when full assets are not available."""
        html_content = f"""<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>FastLED WASM</title>
    <style>
        body {{ margin: 0; background: #000; display: flex; justify-content: center; align-items: center; min-height: 100vh; }}
        canvas {{ border: 1px solid #333; }}
    </style>
</head>
<body>
    <canvas id="canvas" width="800" height="600"></canvas>
    <script type="module">
        import Module from './{module_name}.js';

        async function main() {{
            const module = await Module();

            // Initialize FastLED
            module._extern_setup();

            const canvas = document.getElementById('canvas');
            const ctx = canvas.getContext('2d');

            // Animation loop
            function animate() {{
                module._extern_loop();
                requestAnimationFrame(animate);
            }}

            animate();
        }}

        main().catch(console.error);
    </script>
</body>
</html>
"""
        index_path = output_dir / "index.html"
        index_path.write_text(html_content)

    def check_installation(self) -> bool:
        """Check if Emscripten is properly installed."""
        try:
            self._find_compilers()
            return True
        except FileNotFoundError:
            return False

    def get_version(self) -> str | None:
        """Get Emscripten version if installed."""
        try:
            paths = self._find_compilers()
            emcc_path = paths.emcc
            env = os.environ.copy()

            # Check if this is a clang-tool-chain installation (Python script)
            if emcc_path.suffix == ".py":
                # clang-tool-chain: Run the .py script with Python
                clang_tool_chain_dir = _get_clang_tool_chain_emscripten_dir()
                if clang_tool_chain_dir:
                    env["EMSCRIPTEN"] = str(clang_tool_chain_dir / "emscripten")
                    env["EMSCRIPTEN_ROOT"] = str(clang_tool_chain_dir / "emscripten")
                    config_path = clang_tool_chain_dir / ".emscripten"
                    if config_path.exists():
                        env["EM_CONFIG"] = str(config_path)
                    bin_dir = clang_tool_chain_dir / "bin"
                    if bin_dir.exists():
                        env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"

                cmd = [sys.executable, str(emcc_path), "--version"]
            else:
                cmd = [str(emcc_path), "--version"]

            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                env=env,
            )
            if result.returncode == 0:
                # First line contains version
                return result.stdout.split("\n")[0]
            return None
        except (FileNotFoundError, subprocess.SubprocessError):
            return None
