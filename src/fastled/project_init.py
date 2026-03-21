import io
import json
import re
import shutil
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path

import httpx

from fastled.spinner import Spinner

DEFAULT_URL = "https://fastled.onrender.com"

DEFAULT_EXAMPLE = "wasm"

GITHUB_REPO = "FastLED/FastLED"
GITHUB_RELEASES_API = f"https://api.github.com/repos/{GITHUB_REPO}/releases/latest"

# Sentinel value meaning "use the latest release"
REF_LATEST_RELEASE = "latest_release"


@dataclass
class DownloadResult:
    """Result of downloading the FastLED repo from GitHub."""

    content: bytes | None = None
    error: Exception | None = None

    @property
    def ok(self) -> bool:
        return self.content is not None


def _is_commit_sha(ref: str) -> bool:
    """Check if ref looks like a git commit SHA (7-40 hex chars)."""
    return bool(re.match(r"^[0-9a-fA-F]{7,40}$", ref))


def _get_latest_release_tag() -> str | None:
    """Fetch the latest release tag from GitHub API. Returns None on failure."""
    try:
        response = httpx.get(
            GITHUB_RELEASES_API,
            follow_redirects=True,
            timeout=10,
            headers={"Accept": "application/vnd.github.v3+json"},
        )
        response.raise_for_status()
        return response.json()["tag_name"]
    except Exception:
        return None


def _build_archive_url(ref: str) -> str:
    """Build the GitHub archive URL for a given ref.

    Args:
        ref: A branch name, tag name, or commit SHA.
    """
    base = f"https://github.com/{GITHUB_REPO}/archive"
    if _is_commit_sha(ref):
        return f"{base}/{ref}.zip"
    # Could be a branch or tag — GitHub resolves both via this URL pattern
    return f"{base}/refs/heads/{ref}.zip"


def _build_tag_archive_url(tag: str) -> str:
    """Build the GitHub archive URL for a specific tag."""
    return f"https://github.com/{GITHUB_REPO}/archive/refs/tags/{tag}.zip"


def _resolve_ref(ref: str | None) -> tuple[str, str]:
    """Resolve a ref string into (display_name, archive_url).

    Returns:
        Tuple of (ref_display_name, archive_url).
        ref_display_name is the resolved ref (e.g. "3.9.12" for latest release).
    """
    if ref is None or ref == REF_LATEST_RELEASE:
        tag = _get_latest_release_tag()
        if tag:
            return tag, _build_tag_archive_url(tag)
        # Fallback to master if we can't get latest release
        print("Warning: Could not fetch latest release, falling back to master")
        return "master", _build_archive_url("master")

    if _is_commit_sha(ref):
        return ref, _build_archive_url(ref)

    # Try as a tag first, fall back to branch
    tag_url = _build_tag_archive_url(ref)
    try:
        resp = httpx.head(tag_url, follow_redirects=True, timeout=10)
        if resp.status_code == 200:
            return ref, tag_url
    except Exception:
        pass

    return ref, _build_archive_url(ref)


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


# --- Core download ---


def _find_example_in_repo(repo_root: Path, example: str) -> Path | None:
    """Search for an example directory in the FastLED repo.

    Handles both flat layout (examples/FxSdCard/) and nested layout
    (examples/Fx/FxSdCard/).
    """
    examples_dir = repo_root / "examples"
    if not examples_dir.exists():
        return None
    # Direct match: examples/{example}/
    direct = examples_dir / example
    if direct.exists() and direct.is_dir():
        return direct
    # Nested match: examples/*/{example}/
    for subdir in examples_dir.iterdir():
        if subdir.is_dir():
            nested = subdir / example
            if nested.exists() and nested.is_dir():
                return nested
    return None


def _download_fastled_repo(ref: str | None = None) -> tuple[DownloadResult, str]:
    """Download the FastLED repo zip from GitHub.

    Args:
        ref: Git ref to download. None means latest release.

    Returns:
        Tuple of (DownloadResult, resolved_ref_name).
    """
    ref_name, url = _resolve_ref(ref)
    try:
        response = httpx.get(url, follow_redirects=True, timeout=60)
        response.raise_for_status()
        return DownloadResult(content=response.content), ref_name
    except Exception as e:
        return DownloadResult(error=e), ref_name


def _find_repo_root(tmp_dir: Path) -> Path:
    """Find the extracted FastLED repo root directory."""
    # Try common patterns: FastLED-master, FastLED-3.9.12, FastLED-<sha>, etc.
    dirs = [d for d in tmp_dir.iterdir() if d.is_dir() and d.name.startswith("FastLED")]
    if dirs:
        return dirs[0]
    raise FileNotFoundError("Failed to find FastLED directory in downloaded archive")


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
                # Check nested directories (e.g., examples/Fx/FxSdCard/)
                for nested in entry.iterdir():
                    if nested.is_dir() and list(nested.glob("*.ino")):
                        found.append(nested.name)
    return sorted(found)


def get_examples(host: str | None = None, ref: str | None = None) -> list[str]:
    """Get list of available examples from the FastLED GitHub repo.

    Args:
        host: Fallback server host for example listing.
        ref: Git ref to use. None means latest release.
    """
    # If we're inside the FastLED repo, use local examples
    local_repo = _find_fastled_repo_via_library_json(Path.cwd())
    if local_repo is not None:
        print(f"Using local FastLED repo at {local_repo}")
        return _collect_examples_from_dir(local_repo / "examples")

    print("Fetching examples from FastLED GitHub repo...")
    with Spinner("Downloading FastLED repo..."):
        result, _ = _download_fastled_repo(ref)
    if not result.ok:
        # Fall back to server if GitHub download fails
        fallback_host = host or DEFAULT_URL
        url_info = f"{fallback_host}/info"
        response = httpx.get(url_info, timeout=4)
        response.raise_for_status()
        examples: list[str] = response.json()["examples"]
        return sorted(examples)

    assert result.content is not None
    tmp_dir = Path(tempfile.mkdtemp(prefix="fastled_examples_"))
    try:
        with zipfile.ZipFile(io.BytesIO(result.content)) as zf:
            zf.extractall(tmp_dir)
        repo_root = _find_repo_root(tmp_dir)
        return _collect_examples_from_dir(repo_root / "examples")
    finally:
        shutil.rmtree(tmp_dir, ignore_errors=True)


def _prompt_for_example(ref: str | None = None) -> str:
    from fastled.select_sketch_directory import _disambiguate_user_choice

    examples = get_examples(ref=ref)

    # Find default example index (prefer DEFAULT_EXAMPLE if it exists)
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
        # Fallback to DEFAULT_EXAMPLE if user cancelled
        return DEFAULT_EXAMPLE

    return result


def project_init(
    example: str | None = "PROMPT",  # prompt for example
    outputdir: Path | None = None,
    host: str | None = None,  # noqa: ARG001 - kept for API compatibility
    ref: str | None = None,
) -> Path:
    """Initialize a new FastLED project by downloading the example from GitHub.

    Args:
        example: Example name, "PROMPT" to prompt user, or None for default.
        outputdir: Output directory (defaults to "fastled").
        host: Unused, kept for API compatibility.
        ref: Git ref to download. None means latest release. Use "master" for
             master branch, a branch name, or a commit SHA.
    """
    del host  # unused, kept for API compatibility
    outputdir = Path(outputdir) if outputdir is not None else Path("fastled")
    outputdir.mkdir(exist_ok=True, parents=True)

    # Check if we're inside the FastLED repo — use local files instead
    local_repo = _find_fastled_repo_via_library_json(Path.cwd())
    if local_repo is not None:
        return _init_from_local_repo(local_repo, example, outputdir)

    # Check for saved ref in fastled.json (in the output directory or cwd)
    if ref is None:
        saved_ref = _read_fastled_json(Path.cwd()) or _read_fastled_json(outputdir)
        if saved_ref is not None:
            ref = saved_ref
            print(f"Using saved ref '{ref}' from fastled.json")

    if example == "PROMPT" or example is None:
        try:
            example = _prompt_for_example(ref=ref)
        except Exception:
            print(
                f"Failed to fetch examples, using default example '{DEFAULT_EXAMPLE}'"
            )
            example = DEFAULT_EXAMPLE
    assert example is not None

    ref_display = ref if ref else "latest release"
    print(
        f"Initializing project with example '{example}' from GitHub repo ({ref_display})"
    )

    with Spinner(f"Downloading FastLED repo for '{example}'..."):
        result, resolved_ref = _download_fastled_repo(ref)

    print()  # New line after spinner

    if not result.ok:
        assert result.error is not None
        raise result.error

    assert result.content is not None
    tmp_dir = Path(tempfile.mkdtemp(prefix="fastled_init_"))
    try:
        with zipfile.ZipFile(io.BytesIO(result.content)) as zf:
            zf.extractall(tmp_dir)

        repo_root = _find_repo_root(tmp_dir)

        example_src = _find_example_in_repo(repo_root, example)
        if example_src is None:
            raise FileNotFoundError(f"Example '{example}' not found in FastLED repo")

        dest = outputdir / example
        dest.mkdir(parents=True, exist_ok=True)
        shutil.copytree(example_src, dest, dirs_exist_ok=True)
    finally:
        shutil.rmtree(tmp_dir, ignore_errors=True)

    out = outputdir / example

    # Save fastled.json for non-default refs
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
