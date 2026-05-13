import json
import os
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

from fastled.interrupts import handle_keyboard_interrupt

DEFAULT_EXAMPLE = "wasm"


@dataclass
class _CachedRepo:
    root: Path
    ref_name: str


def _find_rust_fastled_cli() -> Path | None:
    """Locate the **Rust** fastled CLI binary, not the Python entry-point shim.

    The Python `[project.scripts] fastled = ...` console-script sits in the
    same directory as the interpreter and shadows the Rust binary in step 1
    of the open_browser lookup. We need the Rust one, so search the workspace
    target/ tree first, then PATH.
    """
    import shutil
    import sys

    exe_name = "fastled.exe" if sys.platform == "win32" else "fastled"

    # Walk up to find a Cargo.toml workspace root and check target dirs.
    current = Path(__file__).resolve().parent
    for _ in range(10):
        if (current / "Cargo.toml").is_file():
            for profile in ("release", "debug"):
                candidate = current / "target" / profile / exe_name
                if candidate.is_file():
                    return candidate
            target_dir = current / "target"
            if target_dir.is_dir():
                for arch_dir in target_dir.iterdir():
                    if arch_dir.is_dir() and not arch_dir.name.startswith("."):
                        for profile in ("release", "debug"):
                            candidate = arch_dir / profile / exe_name
                            if candidate.is_file():
                                return candidate
            break
        parent = current.parent
        if parent == current:
            break
        current = parent

    found = shutil.which(exe_name)
    if found:
        return Path(found)
    return None


def _ensure_repo_via_rust(ref: str | None) -> Path:
    """Invoke the Rust binary's hidden ``--internal-ensure-fastled-repo`` flag
    to materialise the FastLED repo locally and return the path.

    The Rust side owns the actual GitHub download. Python never performs the
    HTTP request itself.
    """
    cli = _find_rust_fastled_cli()
    if cli is None:
        raise RuntimeError(
            "Could not locate the fastled CLI binary; cannot fetch FastLED repo."
        )

    cmd: list[str] = [str(cli), "--internal-ensure-fastled-repo"]
    if ref is not None:
        cmd.append(ref)

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    except KeyboardInterrupt as ki:
        handle_keyboard_interrupt(ki)
        raise

    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise RuntimeError(
            f"fastled --internal-ensure-fastled-repo failed (exit {result.returncode})"
        )

    repo_path_str = result.stdout.strip().splitlines()[-1] if result.stdout else ""
    repo_path = Path(repo_path_str)
    if not repo_path.is_dir():
        raise RuntimeError(
            f"Rust binary returned non-existent repo path: {repo_path_str!r}"
        )
    return repo_path


def _get_local_fastled_repo(ref: str | None) -> _CachedRepo:
    """Return a path to the locally-cached FastLED repo (and the resolved ref).

    Prefers ``FASTLED_LOCAL_REPO_DIR`` set by the Rust CLI's --init pre-step.
    Falls back to invoking the Rust binary directly so direct Python API
    consumers (Api.project_init, Api.get_examples) still work without httpx.
    """
    env_path = os.environ.get("FASTLED_LOCAL_REPO_DIR")
    if env_path:
        candidate = Path(env_path)
        if candidate.is_dir():
            return _CachedRepo(
                root=candidate,
                ref_name=candidate.name.replace("fastled-", "", 1) or "master",
            )

    repo_root = _ensure_repo_via_rust(ref)
    ref_name = repo_root.name.replace("fastled-", "", 1) or (ref or "master")
    return _CachedRepo(root=repo_root, ref_name=ref_name)


# --- fastled.json persistence ---


def _read_fastled_json(directory: Path) -> str | None:
    """Read the 'ref' field from fastled.json in the given directory."""
    fpath = directory / "fastled.json"
    if not fpath.exists():
        return None
    try:
        data = json.loads(fpath.read_text(encoding="utf-8"))
        return data.get("ref")
    except (json.JSONDecodeError, OSError):
        return None


def _write_fastled_json(directory: Path, ref: str) -> None:
    """Write fastled.json with the given ref."""
    fpath = directory / "fastled.json"
    data: dict[str, str] = {}
    if fpath.exists():
        try:
            data = json.loads(fpath.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            pass
    data["ref"] = ref
    fpath.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


# --- FastLED repo detection ---


def _find_fastled_repo_via_library_json(start: Path) -> Path | None:
    """Walk up from start looking for library.json with name=FastLED.

    Returns the repo root directory, or None.
    """
    current = start.resolve()
    for _ in range(10):  # limit depth
        lib_json = current / "library.json"
        if lib_json.exists():
            try:
                data = json.loads(lib_json.read_text(encoding="utf-8"))
                if data.get("name") == "FastLED":
                    return current
            except (json.JSONDecodeError, OSError):
                pass
        parent = current.parent
        if parent == current:
            break
        current = parent
    return None


# --- Core lookup helpers ---


def _find_example_in_repo(repo_root: Path, example: str) -> Path | None:
    """Search for an example directory in the FastLED repo.

    Handles both flat layout (examples/FxSdCard/) and nested layout
    (examples/Fx/FxSdCard/).
    """
    examples_dir = repo_root / "examples"
    if not examples_dir.exists():
        return None
    direct = examples_dir / example
    if direct.exists() and direct.is_dir():
        return direct
    for subdir in examples_dir.iterdir():
        if subdir.is_dir():
            nested = subdir / example
            if nested.exists() and nested.is_dir():
                return nested
    return None


def _collect_examples_from_dir(examples_dir: Path) -> list[str]:
    """Collect example names from an examples directory."""
    if not examples_dir.exists():
        return []
    found: list[str] = []
    for entry in examples_dir.iterdir():
        if entry.is_dir():
            ino_files = list(entry.glob("*.ino"))
            if ino_files:
                found.append(entry.name)
            else:
                for nested in entry.iterdir():
                    if nested.is_dir() and list(nested.glob("*.ino")):
                        found.append(nested.name)
    return sorted(found)


def get_examples(ref: str | None = None) -> list[str]:
    """Get list of available examples from the FastLED GitHub repo.

    Args:
        ref: Git ref to use. None means latest release.
    """
    local_repo = _find_fastled_repo_via_library_json(Path.cwd())
    if local_repo is not None:
        print(f"Using local FastLED repo at {local_repo}")
        return _collect_examples_from_dir(local_repo / "examples")

    cached = _get_local_fastled_repo(ref)
    return _collect_examples_from_dir(cached.root / "examples")


def _prompt_for_example(ref: str | None = None) -> str:
    from fastled.select_sketch_directory import _disambiguate_user_choice

    examples = get_examples(ref=ref)

    default_index = 0
    if DEFAULT_EXAMPLE in examples:
        default_index = examples.index(DEFAULT_EXAMPLE)

    result = _disambiguate_user_choice(
        examples,
        option_to_str=lambda x: x,
        prompt="Available examples:",
        default_index=default_index,
    )

    if result is None:
        return DEFAULT_EXAMPLE

    return result


def project_init(
    example: str | None = "PROMPT",
    outputdir: Path | None = None,
    ref: str | None = None,
) -> Path:
    """Initialize a new FastLED project from a Rust-cached repo copy."""
    outputdir = Path(outputdir) if outputdir is not None else Path("fastled")
    outputdir.mkdir(exist_ok=True, parents=True)

    local_repo = _find_fastled_repo_via_library_json(Path.cwd())
    if local_repo is not None:
        return _init_from_local_repo(local_repo, example, outputdir)

    if ref is None:
        saved_ref = _read_fastled_json(Path.cwd()) or _read_fastled_json(outputdir)
        if saved_ref is not None:
            ref = saved_ref
            print(f"Using saved ref '{ref}' from fastled.json")

    if example == "PROMPT" or example is None:
        try:
            example = _prompt_for_example(ref=ref)
        except KeyboardInterrupt as ki:
            handle_keyboard_interrupt(ki)
        except Exception:
            print(
                f"Failed to fetch examples, using default example '{DEFAULT_EXAMPLE}'"
            )
            example = DEFAULT_EXAMPLE
    assert example is not None

    ref_display = ref if ref else "latest release"
    print(
        f"Initializing project with example '{example}' from FastLED repo ({ref_display})"
    )

    cached = _get_local_fastled_repo(ref)
    repo_root = cached.root
    resolved_ref = cached.ref_name

    example_src = _find_example_in_repo(repo_root, example)
    if example_src is None:
        raise FileNotFoundError(f"Example '{example}' not found in FastLED repo")

    dest = outputdir / example
    dest.mkdir(parents=True, exist_ok=True)
    shutil.copytree(example_src, dest, dirs_exist_ok=True)

    out = outputdir / example

    if ref is not None:
        _write_fastled_json(out, resolved_ref)
        print(f"Saved ref '{resolved_ref}' to {out / 'fastled.json'}")

    print(f"Project initialized at {out}")
    assert out.exists()
    return out


def _init_from_local_repo(
    repo_root: Path, example: str | None, outputdir: Path
) -> Path:
    """Initialize a project from a local FastLED repo."""
    print(f"Using local FastLED repo at {repo_root}")

    if example == "PROMPT" or example is None:
        examples = _collect_examples_from_dir(repo_root / "examples")
        if not examples:
            raise FileNotFoundError(
                f"No examples found in local FastLED repo at {repo_root}"
            )
        from fastled.select_sketch_directory import _disambiguate_user_choice

        default_index = 0
        if DEFAULT_EXAMPLE in examples:
            default_index = examples.index(DEFAULT_EXAMPLE)
        result = _disambiguate_user_choice(
            examples,
            option_to_str=lambda x: x,
            prompt="Available examples:",
            default_index=default_index,
        )
        example = result if result else DEFAULT_EXAMPLE

    assert example is not None
    example_src = _find_example_in_repo(repo_root, example)
    if example_src is None:
        raise FileNotFoundError(
            f"Example '{example}' not found in local FastLED repo at {repo_root}"
        )

    dest = outputdir / example
    dest.mkdir(parents=True, exist_ok=True)
    shutil.copytree(example_src, dest, dirs_exist_ok=True)

    out = outputdir / example
    print(f"Project initialized at {out}")
    assert out.exists()
    return out


def unit_test() -> None:
    project_init()


if __name__ == "__main__":
    unit_test()
