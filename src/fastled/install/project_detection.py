"""Project detection logic for FastLED installation."""

import json
import shutil
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class LibraryJson:
    name: str | None
    repository_url: str

    @classmethod
    def from_file(cls, path: Path) -> "LibraryJson | None":
        try:
            with open(path, "r") as f:
                data = json.load(f)
        except (json.JSONDecodeError, FileNotFoundError):
            return None
        if not isinstance(data, dict):
            return None
        repository = data.get("repository")
        repository_url = ""
        if isinstance(repository, dict):
            url = repository.get("url")
            repository_url = url if isinstance(url, str) else ""
        name = data.get("name")
        return cls(
            name=name if isinstance(name, str) else None,
            repository_url=repository_url,
        )


def validate_vscode_project(no_interactive: bool = False) -> bool:
    """
    Validate if current directory has a VSCode project.
    If not found, search parent directories, offer alternatives.
    Returns True if a VSCode project is found or created.

    Args:
        no_interactive: If True, fail instead of prompting for input
    """
    current_dir = Path.cwd()

    # Check current directory
    if (current_dir / ".vscode").exists():
        return True

    # Search parent directories
    parent_path = find_vscode_project_upward()
    if parent_path:
        if no_interactive:
            print("❌ No .vscode directory found in current directory.")
            print(f"   Found .vscode in parent: {parent_path}")
            print("   In non-interactive mode, cannot change directory.")
            print(f"   Please cd to {parent_path} and run the command again.")
            return False
        answer = (
            input(f"Found a .vscode project in {parent_path}/\nInstall there? [y/n] ")
            .strip()
            .lower()
        )
        if answer in ["y", "yes"]:
            import os

            os.chdir(parent_path)
            return True

    # Check if IDE is available
    if not (shutil.which("code") or shutil.which("cursor")):
        print(
            "No supported IDE found (VSCode or Cursor). Please install VSCode or Cursor first."
        )
        return False

    # Offer to create new project
    if no_interactive:
        print(
            "❌ No .vscode directory found in current directory or parent directories."
        )
        print("   In non-interactive mode, cannot create new project.")
        print("   Please create a .vscode directory or run without --no-interactive.")
        return False

    print("No .vscode directory found in current directory or parent directories.")
    answer = (
        input(
            "Would you like to generate a VSCode project with FastLED configuration? [y/n] "
        )
        .strip()
        .lower()
    )
    if answer in ["y", "yes"]:
        return generate_vscode_project()

    return False


def find_vscode_project_upward(max_levels: int = 5) -> Path | None:
    """Search parent directories for .vscode folder."""
    current = Path.cwd()

    for _ in range(max_levels):
        parent = current.parent
        if parent == current:  # Reached root
            break
        current = parent
        if (current / ".vscode").exists():
            return current

    return None


def generate_vscode_project() -> bool:
    """Create a new .vscode directory structure."""
    vscode_dir = Path.cwd() / ".vscode"
    vscode_dir.mkdir(exist_ok=True)
    print(f"✅ Created .vscode directory at {vscode_dir}")
    return True


def detect_fastled_project() -> bool:
    """Check if library.json contains FastLED."""
    library_json = Path.cwd() / "library.json"
    metadata = LibraryJson.from_file(library_json)
    return metadata is not None and metadata.name == "FastLED"


def is_fastled_repository() -> bool:
    """
    🚨 CRITICAL: Detect actual FastLED repository.
    Strict verification of multiple markers.
    """
    cwd = Path.cwd()

    # Required files and directories for FastLED repository
    required_markers = [
        cwd / "src" / "FastLED.h",
        cwd / "examples" / "Blink" / "Blink.ino",
        cwd / "ci" / "ci-compile.py",
        cwd / "src" / "platforms",
        cwd / "library.json",
    ]

    # Check all markers exist
    if not all(marker.exists() for marker in required_markers):
        return False

    # Verify library.json has correct content
    metadata = LibraryJson.from_file(cwd / "library.json")
    if metadata is None:
        return False
    if metadata.name != "FastLED":
        return False
    if "FastLED/FastLED" not in metadata.repository_url:
        return False

    # Check for test files pattern
    test_dir = cwd / "tests"
    if test_dir.exists() and test_dir.is_dir():
        test_files = list(test_dir.glob("test_*.cpp"))
        if not test_files:
            return False
    else:
        return False

    return True


def check_existing_arduino_content() -> bool:
    """Check for .ino files OR examples/ folder."""
    cwd = Path.cwd()

    # Check for any .ino files
    ino_files = list(cwd.rglob("*.ino"))
    if ino_files:
        return True

    # Check for examples folder
    if (cwd / "examples").exists():
        return True

    return False
