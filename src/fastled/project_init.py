import os
from dataclasses import dataclass
from pathlib import Path

from fastled._native import collect_examples as _native_collect_examples
from fastled._native import ensure_fastled_repo as _native_ensure_fastled_repo
from fastled._native import (
    find_fastled_repo_upwards as _native_find_fastled_repo_upwards,
)
from fastled._native import init_example_from_repo as _native_init_example_from_repo
from fastled._native import read_fastled_json_ref as _native_read_fastled_json_ref
from fastled.interrupts import handle_keyboard_interrupt

DEFAULT_EXAMPLE = "wasm"


@dataclass
class _CachedRepo:
    root: Path
    ref_name: str


def _ensure_repo_via_rust(ref: str | None) -> Path:
    """Materialise the FastLED repo locally via the native Rust extension and
    return the path.

    The Rust side owns the actual GitHub download. Python never performs the
    HTTP request itself.
    """
    repo_path = Path(_native_ensure_fastled_repo(ref))
    if not repo_path.is_dir():
        raise RuntimeError(
            f"Rust extension returned non-existent repo path: {str(repo_path)!r}"
        )
    return repo_path


def _get_local_fastled_repo(ref: str | None) -> _CachedRepo:
    """Return a path to the locally-cached FastLED repo (and the resolved ref).

    Prefers ``FASTLED_LOCAL_REPO_DIR`` set by the Rust CLI's --init pre-step.
    Falls back to invoking the native Rust helper directly so direct Python API
    consumers (Api.project_init, Api.get_examples) still work.
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
    return _native_read_fastled_json_ref(str(directory))


# --- FastLED repo detection ---


def _find_fastled_repo_via_library_json(start: Path) -> Path | None:
    """Walk up from start looking for library.json with name=FastLED.

    Returns the repo root directory, or None.
    """
    found = _native_find_fastled_repo_upwards(str(start), 10)
    return Path(found) if found else None


# --- Core lookup helpers ---


def _collect_examples_from_dir(examples_dir: Path) -> list[str]:
    """Collect example names from an examples directory."""
    return list(_native_collect_examples(str(examples_dir)))


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

    out = _init_example_from_repo(
        repo_root=repo_root,
        example=example,
        outputdir=outputdir,
        resolved_ref=resolved_ref if ref is not None else None,
    )

    if ref is not None:
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
    out = _init_example_from_repo(
        repo_root=repo_root,
        example=example,
        outputdir=outputdir,
        resolved_ref=None,
    )
    print(f"Project initialized at {out}")
    assert out.exists()
    return out


def _init_example_from_repo(
    repo_root: Path,
    example: str,
    outputdir: Path,
    resolved_ref: str | None,
) -> Path:
    return Path(
        _native_init_example_from_repo(
            str(repo_root), example, str(outputdir), resolved_ref
        )
    )


def unit_test() -> None:
    project_init()


if __name__ == "__main__":
    unit_test()
