# A compilation script specific to fastled's docker compiler.
# This script will pull the users code from a mapped directory,
# then do some processing to convert the *.ino files to *.cpp and
# insert certain headers like "Arduino.h" (pointing to a fake implementation).
# After this, the code is compiled, and the output files are copied back
# to the users mapped directory in the fastled_js folder.
# There are a few assumptions for this script:
# 1. The mapped directory will contain only one directory with the users code, this is
#    enforced by the script that sets up the docker container.
# 2. The docker container has installed compiler dependencies in the /js directory.


print("Compiler script starting...")

import argparse  # noqa: E402
import hashlib  # noqa: E402
import json  # noqa: E402
import os  # noqa: E402
import re  # noqa: E402
import shutil  # noqa: E402
import subprocess  # noqa: E402
import sys  # noqa: E402
import traceback  # noqa: E402
from dataclasses import dataclass  # noqa: E402
from datetime import datetime  # noqa: E402
from enum import Enum  # noqa: E402
from pathlib import Path  # noqa: E402
from typing import List  # noqa: E402

from paths import (  # noqa: E402
    COMPILER_ROOT,
    FASTLED_COMPILER_DIR,
    PIO_BUILD_DIR,
    SKETCH_SRC,
)

print("Finished imports...")

_FASTLED_MODULES_DIR = FASTLED_COMPILER_DIR / "modules"
_INDEX_HTML_SRC = FASTLED_COMPILER_DIR / "index.html"
_INDEX_CSS_SRC = FASTLED_COMPILER_DIR / "index.css"
_INDEX_JS_SRC = FASTLED_COMPILER_DIR / "index.js"
_PIO_VERBOSE = True


_WASM_COMPILER_SETTTINGS = FASTLED_COMPILER_DIR / "wasm_compiler_flags.py"
# _OUTPUT_FILES = ["fastled.js", "fastled.wasm"]
_HEADERS_TO_INSERT = ["#include <Arduino.h>", '#include "platforms/wasm/js.h"']
_FILE_EXTENSIONS = [".ino", ".h", ".hpp", ".cpp"]
# _MAX_COMPILE_ATTEMPTS = 1  # Occasionally the compiler fails for unknown reasons, but disabled because it increases the build time on failure.
_FASTLED_OUTPUT_DIR_NAME = "fastled_js"


@dataclass
class DateLine:
    dt: datetime
    line: str


class BuildMode(Enum):
    DEBUG = "DEBUG"
    QUICK = "QUICK"
    RELEASE = "RELEASE"

    @classmethod
    def from_string(cls, mode_str: str) -> "BuildMode":
        try:
            return cls[mode_str.upper()]
        except KeyError:
            valid_modes = [mode.name for mode in cls]
            raise ValueError(f"BUILD_MODE must be one of {valid_modes}, got {mode_str}")


@dataclass
class SyntaxCheckResult:
    file_path: Path
    is_valid: bool
    message: str


def copy_files(src_dir: Path, js_src: Path) -> None:
    print("Copying files from mapped directory to container...")
    for item in src_dir.iterdir():
        if item.is_dir():
            print(f"Copying directory: {item}")
            shutil.copytree(item, js_src / item.name, dirs_exist_ok=True)
        else:
            print(f"Copying file: {item}")
            shutil.copy2(item, js_src / item.name)


def _banner(msg: str) -> str:
    """
    Create a banner for the given message.
    Example:
    msg = "Hello, World!"
    print -> "#################"
             "# Hello, World! #"
             "#################"
    """
    lines = msg.split("\n")
    # Find the width of the widest line
    max_width = max(len(line) for line in lines)
    width = max_width + 4  # Add 4 for "# " and " #"

    # Create the top border
    banner = "\n" + "#" * width + "\n"

    # Add each line with proper padding
    for line in lines:
        padding = max_width - len(line)
        banner += f"# {line}{' ' * padding} #\n"

    # Add the bottom border
    banner += "#" * width + "\n"
    return banner


def _chunked_print(text: str, lines_per_print: int = 10) -> None:
    """Prints the text in chunks of the specified size."""
    lines = text.splitlines()
    buffer: list[str] = []

    def flush() -> None:
        if buffer:
            print("\n".join(buffer))
            buffer.clear()

    for line in lines:
        buffer.append(line)
        if len(buffer) >= lines_per_print:
            flush()
    flush()


def compile(
    compiler_root: Path, build_mode: BuildMode, auto_clean: bool, no_platformio: bool
) -> int:
    print("Starting compilation process...")
    max_attempts = 1
    env = os.environ.copy()
    env["BUILD_MODE"] = build_mode.name
    print(_banner(f"WASM is building in mode: {build_mode.name}"))
    cmd_list: list[str] = []
    if no_platformio:
        # execute build_archive.syh
        cmd_list = [
            "/bin/bash",
            "-c",
            (compiler_root / "build_fast.sh").as_posix(),
        ]
    else:
        cmd_list.extend(["pio", "run"])
        if not auto_clean:
            cmd_list.append("--disable-auto-clean")
        if _PIO_VERBOSE:
            cmd_list.append("-v")

    def _open_process(cmd_list: list[str] = cmd_list) -> subprocess.Popen:
        print(
            _banner(
                "Build started with command:\n  " + subprocess.list2cmdline(cmd_list)
            )
        )
        out = subprocess.Popen(
            cmd_list,
            cwd=compiler_root,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            universal_newlines=True,
            env=env,
        )
        return out

    output_lines = []
    for attempt in range(1, max_attempts + 1):
        try:
            print(f"Attempting compilation (attempt {attempt}/{max_attempts})...")
            process = _open_process()
            assert process.stdout is not None
            line: str
            for line in process.stdout:
                timestamped_line = _timestamp_output(line)
                output_lines.append(timestamped_line)
            process.wait()
            relative_output = _make_timestamps_relative("\n".join(output_lines))
            _chunked_print(relative_output)
            if process.returncode == 0:
                print(_banner(f"Compilation successful on attempt {attempt}"))
                return 0
            else:
                raise subprocess.CalledProcessError(process.returncode, ["pio", "run"])
        except subprocess.CalledProcessError:
            print(f"Compilation failed on attempt {attempt}")
            if attempt == max_attempts:
                print("Max attempts reached. Compilation failed.")
                return 1
            print("Retrying...")
    return 1


def insert_header(file: Path) -> None:
    print(f"Inserting header in file: {file}")
    with open(file, "r") as f:
        content = f.read()

    # Remove existing includes
    for header in _HEADERS_TO_INSERT:
        content = re.sub(
            rf"^.*{re.escape(header)}.*\n", "", content, flags=re.MULTILINE
        )

    # Remove both versions of Arduino.h include
    arduino_pattern = r'^\s*#\s*include\s*[<"]Arduino\.h[>"]\s*.*\n'
    content = re.sub(arduino_pattern, "", content, flags=re.MULTILINE)

    # Add new headers at the beginning
    content = "\n".join(_HEADERS_TO_INSERT) + "\n" + content

    with open(file, "w") as f:
        f.write(content)
    print(f"Processed: {file}")


def transform_to_cpp(src_dir: Path) -> None:
    print("Transforming files to cpp...")
    ino_files = list(src_dir.glob("*.ino"))

    if ino_files:
        ino_file = ino_files[0]
        print(f"Found .ino file: {ino_file}")
        main_cpp = src_dir / "main.cpp"
        if main_cpp.exists():
            print("main.cpp already exists, renaming to main2.hpp")
            main_cpp.rename(src_dir / "main2.hpp")

        new_cpp_file = ino_file.with_suffix(".ino.cpp")
        print(f"Renaming {ino_file} to {new_cpp_file.name}")
        ino_file.rename(new_cpp_file)

        if (src_dir / "main2.hpp").exists():
            print(f"Including main2.hpp in {new_cpp_file.name}")
            with open(new_cpp_file, "a") as f:
                f.write('#include "main2.hpp"\n')


def insert_headers(
    src_dir: Path, exclusion_folders: List[Path], file_extensions: List[str]
) -> None:
    print(_banner("Inserting headers in source files..."))
    for file in src_dir.rglob("*"):
        if (
            file.suffix in file_extensions
            and not any(folder in file.parents for folder in exclusion_folders)
            and file.name != "Arduino.h"
        ):
            insert_header(file)


def process_ino_files(src_dir: Path) -> None:
    transform_to_cpp(src_dir)
    exclusion_folders: List[Path] = []
    insert_headers(src_dir, exclusion_folders, _FILE_EXTENSIONS)
    print(_banner("Transform to cpp and insert header operations completed."))


def _make_timestamps_relative(stdout: str) -> str:
    def parse(line: str) -> DateLine:
        parts = line.split(" ")
        if len(parts) < 2:
            raise ValueError(f"Invalid line: {line}")

        date_str, time_str = parts[:2]
        rest = " ".join(parts[2:])
        # Parse with microsecond precision
        dt = datetime.strptime(f"{date_str} {time_str}", "%Y-%m-%d %H:%M:%S.%f")
        return DateLine(dt, rest)

    lines = stdout.split("\n")
    if not lines:
        return stdout
    parsed: list[DateLine] = []
    for line in lines:
        if not line.strip():  # Skip empty lines
            continue
        try:
            parsed.append(parse(line))
        except ValueError:
            print(f"Failed to parse line: {line}")
            continue

    if not parsed:
        return stdout

    outlines: list[str] = []
    start_time = parsed[0].dt

    # Calculate relative times with
    for p in parsed:
        delta = p.dt - start_time
        seconds = delta.total_seconds()
        line_str = f"{seconds:3.2f} {p.line}"
        outlines.append(line_str)

    return "\n".join(outlines)


def _timestamp_output(line: str) -> str:
    now = datetime.now()
    timestamp = now.strftime("%Y-%m-%d %H:%M:%S.%f")
    return f"{timestamp} {line.rstrip()}"


@dataclass
class Args:
    mapped_dir: Path
    keep_files: bool
    only_copy: bool
    only_insert_header: bool
    only_compile: bool
    profile: bool
    disable_auto_clean: bool
    no_platformio: bool
    debug: bool
    quick: bool
    release: bool

    def __post_init__(self):
        assert isinstance(self.mapped_dir, Path)
        assert isinstance(self.keep_files, bool)
        assert isinstance(self.only_copy, bool)
        assert isinstance(self.only_insert_header, bool)
        assert isinstance(self.only_compile, bool)
        assert isinstance(self.profile, bool)
        assert isinstance(self.disable_auto_clean, bool)
        assert isinstance(self.no_platformio, bool)
        assert isinstance(self.debug, bool)
        assert isinstance(self.quick, bool)
        assert isinstance(self.release, bool)


def parse_args() -> Args:
    parser = argparse.ArgumentParser(description="Compile FastLED for WASM")
    parser.add_argument(
        "--mapped-dir",
        type=Path,
        default="/mapped",
        help="Directory containing source files (default: /mapped)",
    )
    parser.add_argument(
        "--keep-files", action="store_true", help="Keep source files after compilation"
    )
    parser.add_argument(
        "--only-copy",
        action="store_true",
        help="Only copy files from mapped directory to container",
    )
    parser.add_argument(
        "--only-insert-header",
        action="store_true",
        help="Only insert headers in source files",
    )
    parser.add_argument(
        "--only-compile", action="store_true", help="Only compile the project"
    )
    parser.add_argument(
        "--profile",
        action="store_true",
        help="Enable profiling for compilation to see what's taking so long.",
    )

    parser.add_argument(
        "--disable-auto-clean",
        action="store_true",
        help="Massaive speed improvement to not have to rebuild everything, but flakes out sometimes.",
        default=os.getenv("DISABLE_AUTO_CLEAN", "0") == "1",
    )
    parser.add_argument(
        "--no-platformio",
        action="store_true",
        help="Don't use platformio to compile the project, use the new system of direct emcc calls.",
    )
    # Add mutually exclusive build mode group
    build_mode = parser.add_mutually_exclusive_group()
    build_mode.add_argument("--debug", action="store_true", help="Build in debug mode")
    build_mode.add_argument(
        "--quick",
        action="store_true",
        default=True,
        help="Build in quick mode (default)",
    )
    build_mode.add_argument(
        "--release", action="store_true", help="Build in release mode"
    )

    tmp = parser.parse_args()
    return Args(
        mapped_dir=tmp.mapped_dir,
        keep_files=tmp.keep_files,
        only_copy=tmp.only_copy,
        only_insert_header=tmp.only_insert_header,
        only_compile=tmp.only_compile,
        profile=tmp.profile,
        disable_auto_clean=tmp.disable_auto_clean,
        no_platformio=tmp.no_platformio,
        debug=tmp.debug,
        quick=tmp.quick,
        release=tmp.release,
    )


def find_project_dir(mapped_dir: Path) -> Path:
    mapped_dirs: List[Path] = list(mapped_dir.iterdir())
    if len(mapped_dirs) > 1:
        raise ValueError(
            f"Error: More than one directory found in {mapped_dir}, which are {mapped_dirs}"
        )

    src_dir: Path = mapped_dirs[0]
    return src_dir


def process_compile(
    js_dir: Path, build_mode: BuildMode, auto_clean: bool, no_platformio: bool
) -> None:
    print("Starting compilation...")
    rtn = compile(js_dir, build_mode, auto_clean, no_platformio=no_platformio)
    print(f"Compilation return code: {rtn}")
    if rtn != 0:
        print("Compilation failed.")
        raise RuntimeError("Compilation failed.")
    print(_banner("Compilation successful."))


def cleanup(args: Args, js_src: Path) -> None:
    if not args.keep_files and not (args.only_copy or args.only_insert_header):
        print("Removing temporary source files")
        shutil.rmtree(js_src)
    else:
        print("Keeping temporary source files")


def hash_file(file_path: Path) -> str:
    hasher = hashlib.md5()
    with open(file_path, "rb") as f:
        for chunk in iter(lambda: f.read(4096), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def run(args: Args) -> int:
    check_paths: list[Path] = [
        COMPILER_ROOT,
        _INDEX_HTML_SRC,
        _INDEX_CSS_SRC,
        _INDEX_JS_SRC,
        _WASM_COMPILER_SETTTINGS,
        FASTLED_COMPILER_DIR,
    ]
    missing_paths = [p for p in check_paths if not p.exists()]
    if missing_paths:
        print("The following paths are missing:")
        for p in missing_paths:
            print(p)
        missing_paths_str = ",".join(str(p.as_posix()) for p in missing_paths)
        raise FileNotFoundError(f"Missing required paths: {missing_paths_str}")

    print("Starting FastLED WASM compilation script...")
    print(f"Keep files flag: {args.keep_files}")
    print(f"Using mapped directory: {args.mapped_dir}")

    if args.profile:
        print(_banner("Enabling profiling for compilation."))
        # Profile linking
        os.environ["EMPROFILE"] = "2"
    else:
        print(
            _banner(
                "Build process profiling is disabled\nuse --profile to get metrics on how long the build process took."
            )
        )

    try:

        src_dir = find_project_dir(args.mapped_dir)

        any_only_flags = args.only_copy or args.only_insert_header or args.only_compile

        do_copy = not any_only_flags or args.only_copy
        do_insert_header = not any_only_flags or args.only_insert_header
        do_compile = not any_only_flags or args.only_compile

        if not any_only_flags:
            if SKETCH_SRC.exists():
                shutil.rmtree(SKETCH_SRC)

        SKETCH_SRC.mkdir(parents=True, exist_ok=True)

        if do_copy:
            copy_files(src_dir, SKETCH_SRC)
            if args.only_copy:
                return 0

        if do_insert_header:
            process_ino_files(SKETCH_SRC)
            if args.only_insert_header:
                print("Transform to cpp and insert header operations completed.")
                return 0

        no_platformio: bool = args.no_platformio

        if do_compile:
            try:
                # Determine build mode from args
                if args.debug:
                    build_mode = BuildMode.DEBUG
                elif args.release:
                    build_mode = BuildMode.RELEASE
                else:
                    # Default to QUICK mode if neither debug nor release specified
                    build_mode = BuildMode.QUICK

                process_compile(
                    js_dir=COMPILER_ROOT,
                    build_mode=build_mode,
                    auto_clean=not args.disable_auto_clean,
                    no_platformio=no_platformio,
                )
            except Exception as e:
                print(f"Error: {str(e)}")
                return 1

            def _get_build_dir_platformio() -> Path:
                # First assert there is only one build artifact directory.
                # The name is dynamic: it's your sketch folder name.
                build_dirs = [d for d in PIO_BUILD_DIR.iterdir() if d.is_dir()]
                if len(build_dirs) != 1:
                    raise RuntimeError(
                        f"Expected exactly one build directory in {PIO_BUILD_DIR}, found {len(build_dirs)}: {build_dirs}"
                    )
                build_dir: Path = build_dirs[0]
                return build_dir

            def _get_build_dir_cmake() -> Path:
                return COMPILER_ROOT / "build"

            if no_platformio:
                build_dir = _get_build_dir_cmake()
            else:
                build_dir = _get_build_dir_platformio()

            print(_banner("Copying output files..."))
            out_dir: Path = src_dir / _FASTLED_OUTPUT_DIR_NAME
            out_dir.mkdir(parents=True, exist_ok=True)

            # Copy all fastled.* build artifacts
            for file_path in build_dir.glob("fastled.*"):
                _dst = out_dir / file_path.name
                print(f"Copying {file_path} to {_dst}")
                shutil.copy2(file_path, _dst)

            # Copy static files.
            print(f"Copying {_INDEX_HTML_SRC} to output directory")
            shutil.copy2(_INDEX_HTML_SRC, out_dir / "index.html")
            print(f"Copying {_INDEX_CSS_SRC} to output directory")
            shutil.copy2(_INDEX_CSS_SRC, out_dir / "index.css")

            # copy all js files in _FASTLED_COMPILER_DIR to output directory
            Path(out_dir / "modules").mkdir(parents=True, exist_ok=True)

            # Recursively copy all non-hidden files and directories
            print(f"Copying files from {_FASTLED_MODULES_DIR} to {out_dir / 'modules'}")
            shutil.copytree(
                src=_FASTLED_MODULES_DIR,
                dst=out_dir / "modules",
                dirs_exist_ok=True,
                ignore=shutil.ignore_patterns(".*"),
            )  # Ignore hidden files

            # Now long needed since now we do glob copy.
            # fastled_js_mem = build_dir / "fastled.js.mem"
            # fastled_wasm_map = build_dir / "fastled.wasm.map"
            # fastled_js_symbols = build_dir / "fastled.js.symbols"
            # if fastled_js_mem.exists():
            #     print(f"Copying {fastled_js_mem} to output directory")
            #     shutil.copy2(fastled_js_mem, out_dir / fastled_js_mem.name)
            # if fastled_wasm_map.exists():
            #     print(f"Copying {fastled_wasm_map} to output directory")
            #     shutil.copy2(fastled_wasm_map, out_dir / fastled_wasm_map.name)
            # if fastled_js_symbols.exists():
            #     print(f"Copying {fastled_js_symbols} to output directory")
            # shutil.copy2(fastled_js_symbols, out_dir / fastled_js_symbols.name)

            print("Copying index.js to output directory")
            shutil.copy2(_INDEX_JS_SRC, out_dir / "index.js")
            optional_input_data_dir = src_dir / "data"
            output_data_dir = out_dir / optional_input_data_dir.name

            # Handle data directory if it exists
            manifest: list[dict] = []
            if optional_input_data_dir.exists():
                # Clean up existing output data directory
                if output_data_dir.exists():
                    for _file in output_data_dir.iterdir():
                        _file.unlink()

                # Create output data directory and copy files
                output_data_dir.mkdir(parents=True, exist_ok=True)
                for _file in optional_input_data_dir.iterdir():
                    if _file.is_file():  # Only copy files, not directories
                        filename: str = _file.name
                        if filename.endswith(".embedded.json"):
                            print(_banner("Embedding data file"))
                            filename_no_embedded = filename.replace(
                                ".embedded.json", ""
                            )
                            # read json file
                            with open(_file, "r") as f:
                                data = json.load(f)
                            hash_value = data["hash"]
                            size = data["size"]
                            manifest.append(
                                {
                                    "name": filename_no_embedded,
                                    "path": f"data/{filename_no_embedded}",
                                    "size": size,
                                    "hash": hash_value,
                                }
                            )
                        else:
                            print(f"Copying {_file.name} -> {output_data_dir}")
                            shutil.copy2(_file, output_data_dir / _file.name)
                            hash = hash_file(_file)
                            manifest.append(
                                {
                                    "name": _file.name,
                                    "path": f"data/{_file.name}",
                                    "size": _file.stat().st_size,
                                    "hash": hash,
                                }
                            )

            # Write manifest file even if empty
            print(_banner("Writing manifest files.json"))
            manifest_json_str = json.dumps(manifest, indent=2, sort_keys=True)
            with open(out_dir / "files.json", "w") as f:
                f.write(manifest_json_str)
        cleanup(args, SKETCH_SRC)

        print(_banner("Compilation process completed successfully"))
        return 0

    except Exception as e:

        stacktrace = traceback.format_exc()
        print(stacktrace)
        print(f"Error: {str(e)}")
        return 1


def main() -> int:
    args = parse_args()
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
