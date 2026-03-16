"""
Emscripten Toolchain for FastLED

Provides native compilation using the Emscripten SDK (EMSDK) for compiling
FastLED sketches to WebAssembly without Docker.

When a local FastLED repo is detected (with ci/wasm_build.py), delegates to
the repo's own build system which uses Meson+Ninja with command capture and
caching via clang-tool-chain for fast incremental rebuilds.

Otherwise falls back to a direct em++ invocation.
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
    """Get the clang-tool-chain Emscripten installation directory."""
    env_path = os.environ.get("CLANG_TOOL_CHAIN_DOWNLOAD_PATH")
    base_dir = Path(env_path) if env_path else Path.home() / ".clang-tool-chain"
    emscripten_base = base_dir / "emscripten"

    if not emscripten_base.exists():
        return None

    if (emscripten_base / "emscripten" / "emcc.py").exists():
        return emscripten_base

    for subdir in emscripten_base.iterdir():
        if subdir.is_dir():
            for arch_dir in subdir.iterdir():
                if arch_dir.is_dir():
                    if (arch_dir / "emscripten" / "emcc.py").exists():
                        return arch_dir
            if (subdir / "emscripten" / "emcc.py").exists():
                return subdir

    return None


def ensure_clang_tool_chain_emscripten() -> Path | None:
    """Ensure clang-tool-chain Emscripten is installed."""
    existing_dir = _get_clang_tool_chain_emscripten_dir()
    if existing_dir:
        return existing_dir

    try:
        import inspect

        from clang_tool_chain.installers.emscripten import (  # type: ignore[import-not-found]
            ensure_emscripten_available,
            get_emscripten_install_dir,
        )

        sig = inspect.signature(ensure_emscripten_available)
        if len(sig.parameters) == 0:
            ensure_emscripten_available()  # type: ignore[call-arg]
            return get_emscripten_install_dir()  # type: ignore[call-arg]
        else:
            return None
    except ImportError:
        return None
    except Exception as e:
        print(f"Warning: Failed to ensure clang-tool-chain Emscripten: {e}")
        return None


def _setup_emscripten_env(env: dict[str, str]) -> None:
    """Set up Emscripten environment variables like ci/wasm_tools.py does."""
    clang_tool_chain_dir = _get_clang_tool_chain_emscripten_dir()
    if not clang_tool_chain_dir:
        return

    emscripten_dir = clang_tool_chain_dir / "emscripten"
    config_path = clang_tool_chain_dir / ".emscripten"
    bin_dir = clang_tool_chain_dir / "bin"

    if emscripten_dir.exists():
        env["EMSCRIPTEN"] = str(emscripten_dir)
        env["EMSCRIPTEN_ROOT"] = str(emscripten_dir)
    if config_path.exists():
        env["EM_CONFIG"] = str(config_path)
    env["EMSDK_PYTHON"] = sys.executable
    env["EMCC_SKIP_SANITY_CHECK"] = "1"
    if bin_dir.exists():
        env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"


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
    compile_flags: tuple[str, ...] = (
        "-DFASTLED_ENGINE_EVENTS_MAX_LISTENERS=50",
        "-DFASTLED_FORCE_NAMESPACE=1",
        "-DFASTLED_USE_PROGMEM=0",
        "-DUSE_OFFSET_CONVERTER=0",
        "-DGL_ENABLE_GET_PROC_ADDRESS=0",
        "-D_REENTRANT=1",
        "-DEMSCRIPTEN_HAS_UNBOUND_TYPE_NAMES=0",
        "-DSKETCH_COMPILE=1",
        "-DFASTLED_WASM_USE_CCALL",
        "-std=gnu++17",
        "-fpermissive",
        "-Wno-constant-logical-operand",
        "-Wnon-c-typedef-for-linkage",
        "-Werror=bad-function-cast",
        "-Werror=cast-function-type",
        "-fno-threadsafe-statics",
        "-fno-exceptions",
        "-fno-rtti",
        "-pthread",
        "-fpch-instantiate-templates",
    )
    link_flags: tuple[str, ...] = (
        "-sWASM=1",
        "-pthread",
        "-sUSE_PTHREADS=1",
        "-sPROXY_TO_PTHREAD",
        "-sMODULARIZE=1",
        "-sEXPORT_NAME=fastled",
        "-sALLOW_MEMORY_GROWTH=1",
        "-sINITIAL_MEMORY=134217728",
        "-sAUTO_NATIVE_LIBRARIES=0",
        "-sEXPORTED_RUNTIME_METHODS=['ccall','cwrap','stringToUTF8','UTF8ToString','lengthBytesUTF8','HEAPU8','getValue']",
        "-sEXPORTED_FUNCTIONS=['_malloc','_free','_main','_extern_setup','_extern_loop','_fastled_declare_files','_getStripPixelData','_getFrameData','_getScreenMapData','_freeFrameData','_getFrameVersion','_hasNewFrameData','_js_fetch_success_callback','_js_fetch_error_callback']",
        "-sEXIT_RUNTIME=0",
        "-sFILESYSTEM=0",
        "-sERROR_ON_UNDEFINED_SYMBOLS=0",
    )

    @property
    def common_flags(self) -> tuple[str, ...]:
        return self.compile_flags + self.link_flags

    def get_optimization_flags(self) -> list[str]:
        if self.build_mode == BuildMode.DEBUG:
            return ["-g", "-O0", "-sASSERTIONS=2"]
        elif self.build_mode == BuildMode.QUICK:
            return ["-O1", "-sASSERTIONS=0"]
        else:
            return ["-O3", "-flto", "-sASSERTIONS=0"]


class EmscriptenToolchain:
    """Emscripten toolchain for compiling FastLED sketches to WebAssembly.

    When a FastLED repo with ci/wasm_build.py is available, delegates to
    that build system for proper compilation with command capture/caching.
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
        self._fastled_path: Path | None = Path(fastled_path) if fastled_path else None
        self._emsdk_path: Path | None = Path(emsdk_path) if emsdk_path else None
        self._compiler_paths: CompilerPaths | None = None
        self._temp_dir: Path | None = None

    def _find_emsdk(self) -> Path | None:
        """Find EMSDK installation path."""
        clang_tool_chain_dir = _get_clang_tool_chain_emscripten_dir()
        if clang_tool_chain_dir and clang_tool_chain_dir.exists():
            if (clang_tool_chain_dir / "emscripten").exists():
                return clang_tool_chain_dir

        emsdk_env = os.environ.get("EMSDK")
        if emsdk_env:
            return Path(emsdk_env)

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
        """Find Emscripten compiler executables."""
        clang_tool_chain_dir = ensure_clang_tool_chain_emscripten()
        if clang_tool_chain_dir and clang_tool_chain_dir.exists():
            emscripten_scripts_dir = clang_tool_chain_dir / "emscripten"
            if emscripten_scripts_dir.exists():
                emcc_path = emscripten_scripts_dir / "emcc.py"
                empp_path = emscripten_scripts_dir / "em++.py"
                emar_path = emscripten_scripts_dir / "emar.py"
                if emcc_path.exists() and empp_path.exists():
                    return CompilerPaths(
                        emcc=emcc_path,
                        empp=empp_path,
                        emar=emar_path if emar_path.exists() else emcc_path,
                    )

        for name_emcc, name_empp, name_emar in [
            ("clang-tool-chain-emcc", "clang-tool-chain-em++", "clang-tool-chain-emar"),
        ]:
            ctc_emcc = shutil.which(name_emcc)
            ctc_empp = shutil.which(name_empp)
            ctc_emar = shutil.which(name_emar)
            if ctc_emcc and ctc_empp:
                return CompilerPaths(
                    emcc=Path(ctc_emcc),
                    empp=Path(ctc_empp),
                    emar=Path(ctc_emar) if ctc_emar else Path(ctc_emcc),
                )

        emcc = shutil.which("emcc")
        empp = shutil.which("em++")
        emar = shutil.which("emar")
        if emcc and empp and emar:
            return CompilerPaths(emcc=Path(emcc), empp=Path(empp), emar=Path(emar))

        emsdk = self._emsdk_path or self._find_emsdk()
        if emsdk:
            if (emsdk / "emscripten").exists() and not (emsdk / "upstream").exists():
                emscripten_scripts_dir = emsdk / "emscripten"
                emcc_path = emscripten_scripts_dir / "emcc.py"
                empp_path = emscripten_scripts_dir / "em++.py"
                emar_path = emscripten_scripts_dir / "emar.py"
            else:
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

        with zipfile.ZipFile(io.BytesIO(response.content)) as zf:
            zf.extractall(target_dir)

        fastled_dir = target_dir / "FastLED-master"
        if not fastled_dir.exists():
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

        if self._temp_dir is None:
            self._temp_dir = Path(tempfile.mkdtemp(prefix="fastled_native_"))

        fastled_dir = self._download_fastled(self._temp_dir)
        self._fastled_path = fastled_dir
        return fastled_dir

    def _has_wasm_build_system(self, fastled_dir: Path) -> bool:
        """Check if the FastLED repo has ci/wasm_build.py."""
        return (fastled_dir / "ci" / "wasm_build.py").exists()

    def _compile_via_wasm_build(
        self,
        sketch_dir: Path,
        output_dir: Path,
        fastled_dir: Path,
        build_mode: BuildMode,
    ) -> Path:
        """Delegate to FastLED's ci/wasm_build.py which uses Meson+Ninja
        with command capture/caching via clang-tool-chain.

        This is the preferred path — it matches the upstream build exactly
        and gets fast incremental rebuilds for free.
        """
        # Map BuildMode to wasm_build mode string
        mode_map = {
            BuildMode.DEBUG: "debug",
            BuildMode.QUICK: "quick",
            BuildMode.RELEASE: "release",
        }
        mode = mode_map[build_mode]

        # wasm_build.py expects an example name and output path.
        # For arbitrary sketch dirs we create a temporary example symlink.
        sketch_name = sketch_dir.name
        example_dir = fastled_dir / "examples" / sketch_name

        # If sketch_dir IS already inside the FastLED examples tree, use it directly
        try:
            sketch_dir.relative_to(fastled_dir / "examples")
            is_in_tree = True
        except ValueError:
            is_in_tree = False

        output_js = output_dir / "fastled.js"

        # wasm_build.py uses relative imports (from ci.wasm_flags import ...)
        # so it must be invoked via `uv run python` from the FastLED repo root.
        uv = shutil.which("uv")
        if uv:
            cmd = [
                uv,
                "run",
                "python",
                str(fastled_dir / "ci" / "wasm_build.py"),
                "--example",
                sketch_name,
                "-o",
                str(output_js),
                "--mode",
                mode,
            ]
        else:
            cmd = [
                sys.executable,
                str(fastled_dir / "ci" / "wasm_build.py"),
                "--example",
                sketch_name,
                "-o",
                str(output_js),
                "--mode",
                mode,
            ]

        env = os.environ.copy()
        _setup_emscripten_env(env)

        if not is_in_tree:
            # Create a temporary symlink so wasm_build.py can find the sketch
            if not example_dir.exists():
                example_dir.symlink_to(sketch_dir, target_is_directory=True)
            needs_cleanup = True
        else:
            needs_cleanup = False

        try:
            print(f"Delegating to FastLED build system (mode: {mode})...")
            result = subprocess.run(
                cmd,
                cwd=str(fastled_dir),
                env=env,
            )
            if result.returncode != 0:
                raise RuntimeError(
                    f"FastLED build system failed with return code {result.returncode}"
                )
        finally:
            if needs_cleanup and example_dir.is_symlink():
                example_dir.unlink()

        return output_js

    def _uses_unity_build(self, fastled_dir: Path) -> bool:
        """Check if the FastLED repo uses unity build pattern."""
        src_dir = fastled_dir / "src"
        return (src_dir / "_build.cpp").exists() and (
            src_dir / "fl" / "_build.cpp"
        ).exists()

    def _create_wasm_platform_build(self, build_dir: Path) -> Path:
        """Create a custom platforms unity build that only includes wasm/stub/shared."""
        content = """// Auto-generated platform build for WASM native compilation
// Only includes wasm, stub, and shared platform sources

#include "FastLED.h"

// Platform root-level sources (guarded by #ifdef, safe for all platforms)
#include "platforms/compile_test.cpp.hpp"
#include "platforms/debug_setup.cpp.hpp"
#include "platforms/ota.cpp.hpp"
#include "platforms/stub_main.cpp.hpp"

// WASM-relevant platform subdirectories only
#include "platforms/shared/_build.cpp.hpp"
#include "platforms/stub/_build.cpp.hpp"
#include "platforms/wasm/_build.cpp.hpp"
"""
        platform_build = build_dir / "wasm_platforms_build.cpp"
        platform_build.write_text(content)
        return platform_build

    def _get_wasm_sources(self, fastled_dir: Path) -> list[Path]:
        """Get FastLED WASM platform source files."""
        src_dir = fastled_dir / "src"
        sources: list[Path] = []
        for d in ["platforms/wasm", "platforms/stub", "platforms/shared"]:
            p = src_dir / d
            if p.exists():
                sources.extend(p.rglob("*.cpp"))
        return sources

    def _get_fastled_core_sources(self, fastled_dir: Path) -> list[Path]:
        """Get FastLED core source files (excluding platform-specific)."""
        src_dir = fastled_dir / "src"
        if not src_dir.exists():
            raise FileNotFoundError(f"FastLED src directory not found at {src_dir}")

        sources: list[Path] = []
        for pattern in ["*.cpp", "*.c"]:
            sources.extend(src_dir.glob(pattern))

        for subdir in ["fl", "fx"]:
            d = src_dir / subdir
            if d.exists():
                for pattern in ["*.cpp", "*.c"]:
                    sources.extend(d.rglob(pattern))

        return sources

    def _get_sketch_sources(self, sketch_dir: Path) -> list[Path]:
        """Get sketch source files (.ino, .cpp, .c)."""
        sources: list[Path] = []
        for ino_file in sketch_dir.glob("*.ino"):
            sources.append(ino_file)
        for pattern in ["*.cpp", "*.c"]:
            sources.extend(sketch_dir.glob(pattern))
        return sources

    def _create_sketch_wrapper(
        self, sketch_sources: list[Path], output_dir: Path
    ) -> Path:
        """Create sketch.cpp wrapper."""
        sketch_content = ""
        for src in sketch_sources:
            if src.suffix == ".ino":
                sketch_content += f"\n// From {src.name}\n"
                sketch_content += src.read_text()

        wrapper_content = f"""
// Auto-generated sketch wrapper for FastLED WASM native compilation
#include "FastLED.h"
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
        """Compile a FastLED sketch to WebAssembly.

        If the FastLED repo has ci/wasm_build.py, delegates to that build
        system for proper Meson+Ninja compilation with command capture and
        caching. Otherwise falls back to a single-pass em++ invocation.
        """
        if self._compiler_paths is None:
            self._compiler_paths = self._find_compilers()

        fastled_dir = self._get_fastled_path()
        print(f"Using FastLED library at: {fastled_dir}")

        output_dir.mkdir(parents=True, exist_ok=True)

        # Preferred path: delegate to FastLED's build system
        if self._has_wasm_build_system(fastled_dir):
            output_js = self._compile_via_wasm_build(
                sketch_dir, output_dir, fastled_dir, build_mode
            )

            # Copy frontend assets
            print("Copying frontend assets...")
            self._copy_frontend_assets(output_dir, fastled_dir)

            self._cleanup_temp()

            wasm_file = output_dir / "fastled.wasm"
            print("Compilation successful!")
            print(f"  JS:   {output_js}")
            print(f"  WASM: {wasm_file}")
            return output_js

        # Fallback: single-pass em++ (no caching, for repos without ci/wasm_build.py)
        return self._compile_fallback(
            sketch_dir, output_dir, fastled_dir, build_mode, profile
        )

    def _compile_fallback(
        self,
        sketch_dir: Path,
        output_dir: Path,
        fastled_dir: Path,
        build_mode: BuildMode,
        profile: bool,
    ) -> Path:
        """Fallback single-pass em++ compilation for repos without ci/wasm_build.py."""
        assert self._compiler_paths is not None
        config = EmscriptenConfig(build_mode=build_mode)

        build_dir = output_dir / ".build"
        build_dir.mkdir(exist_ok=True)

        sketch_sources = self._get_sketch_sources(sketch_dir)
        if not sketch_sources:
            raise FileNotFoundError(f"No sketch files found in {sketch_dir}")

        sketch_file = self._create_sketch_wrapper(sketch_sources, build_dir)

        include_paths = [
            f"-I{fastled_dir}/src",
            f"-I{fastled_dir}/src/platforms/wasm",
            f"-I{fastled_dir}/src/platforms/wasm/compiler",
            f"-I{fastled_dir}/src/platforms/stub",
            f"-I{fastled_dir}/src/platforms/shared",
            f"-I{sketch_dir}",
        ]

        output_file = output_dir / f"{config.output_name}.js"

        if self._uses_unity_build(fastled_dir):
            src_dir = fastled_dir / "src"
            all_sources: list[Path] = [
                src_dir / "_build.cpp",
                src_dir / "fl" / "_build.cpp",
                src_dir / "third_party" / "_build.cpp",
                self._create_wasm_platform_build(build_dir),
            ]
            all_sources = [s for s in all_sources if s.exists()]
            print(
                f"Compiling with {len(sketch_sources)} sketch files using unity build ({len(all_sources)} build units)..."
            )
        else:
            core_sources = self._get_fastled_core_sources(fastled_dir)
            wasm_sources = self._get_wasm_sources(fastled_dir)
            all_sources = core_sources + wasm_sources
            print(
                f"Compiling with {len(sketch_sources)} sketch files and {len(all_sources)} FastLED files..."
            )

        response_file = build_dir / "compile_args.rsp"

        def to_posix_path(p: str) -> str:
            return p.replace("\\", "/")

        # Add --js-library for JS bindings
        js_library = (
            fastled_dir / "src" / "platforms" / "wasm" / "compiler" / "js_library.js"
        )
        extra_link_flags: list[str] = []
        if js_library.exists():
            extra_link_flags.append(f"--js-library={to_posix_path(str(js_library))}")

        response_content_lines = [
            *[to_posix_path(p) for p in include_paths],
            *config.common_flags,
            *config.get_optimization_flags(),
            *extra_link_flags,
            "-o",
            to_posix_path(str(output_file)),
            to_posix_path(str(sketch_file)),
            *[to_posix_path(str(s)) for s in all_sources],
        ]
        response_file.write_text("\n".join(response_content_lines))

        empp_path = self._compiler_paths.empp
        env = os.environ.copy()
        _setup_emscripten_env(env)

        cmd = self._build_emcc_cmd(empp_path, response_file, env)

        if profile:
            print(f"Response file: {response_file}")
            print(f"Compile command: {' '.join(cmd)}")

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

        print("Copying frontend assets...")
        self._copy_frontend_assets(output_dir, fastled_dir)

        if build_dir.exists():
            shutil.rmtree(build_dir, ignore_errors=True)

        self._cleanup_temp()

        wasm_file = output_dir / f"{config.output_name}.wasm"
        print("Compilation successful!")
        print(f"  JS:   {output_file}")
        print(f"  WASM: {wasm_file}")
        return output_file

    def _build_emcc_cmd(
        self, empp_path: Path, response_file: Path, env: dict[str, str]
    ) -> list[str]:
        """Build the em++ command list."""
        if empp_path.suffix == ".py":
            return [sys.executable, str(empp_path), f"@{response_file}"]
        else:
            return [str(empp_path), f"@{response_file}"]

    def _cleanup_temp(self) -> None:
        """Cleanup temp directory if it was created for downloaded FastLED."""
        if self._temp_dir and self._temp_dir.exists():
            shutil.rmtree(self._temp_dir, ignore_errors=True)
            self._temp_dir = None

    def _copy_frontend_assets(self, output_dir: Path, fastled_dir: Path) -> None:
        """Copy Vite-built frontend assets from FastLED's wasm compiler directory."""
        compiler_dir = fastled_dir / "src" / "platforms" / "wasm" / "compiler"
        dist_dir = compiler_dir / "dist"

        if not compiler_dir.exists():
            print(
                f"Warning: Frontend assets not found at {compiler_dir}, using minimal index.html"
            )
            self._create_minimal_index_html(output_dir, "fastled")
            return

        if not dist_dir.exists():
            npx = shutil.which("npx")
            if not npx:
                print(
                    "Warning: Node.js not found. Cannot build frontend. Using minimal index.html"
                )
                self._create_minimal_index_html(output_dir, "fastled")
                return

            if not (compiler_dir / "node_modules").exists():
                print("Installing frontend dependencies...")
                subprocess.run(["npm", "install"], cwd=str(compiler_dir), check=True)

            print("Building frontend with Vite...")
            result = subprocess.run(
                [npx, "vite", "build"],
                cwd=str(compiler_dir),
                capture_output=True,
                text=True,
            )
            if result.returncode != 0:
                print(f"Warning: Vite build failed: {result.stderr}")
                self._create_minimal_index_html(output_dir, "fastled")
                return

        if not dist_dir.exists():
            self._create_minimal_index_html(output_dir, "fastled")
            return

        print("Copying Vite build output...")
        for item in dist_dir.iterdir():
            dest = output_dir / item.name
            if item.is_dir():
                if dest.exists():
                    shutil.rmtree(dest)
                shutil.copytree(item, dest)
            else:
                shutil.copy2(item, dest)

        files_json = output_dir / "files.json"
        if not files_json.exists():
            files_json.write_text("[]")

        print("  Frontend assets copied from Vite build output")

    def _create_minimal_index_html(self, output_dir: Path, module_name: str) -> None:
        """Create a minimal fallback index.html."""
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
            module._extern_setup();

            const canvas = document.getElementById('canvas');
            const ctx = canvas.getContext('2d');

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
            _setup_emscripten_env(env)

            if emcc_path.suffix == ".py":
                cmd = [sys.executable, str(emcc_path), "--version"]
            else:
                cmd = [str(emcc_path), "--version"]

            result = subprocess.run(cmd, capture_output=True, text=True, env=env)
            if result.returncode == 0:
                return result.stdout.split("\n")[0]
            return None
        except (FileNotFoundError, subprocess.SubprocessError):
            return None
