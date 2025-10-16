import os
from pathlib import Path

_MAX_FILES_SEARCH_LIMIT = 10000


def find_sketch_directories(directory: Path | None = None) -> list[Path]:
    if directory is None:
        directory = Path(".")
    file_count = 0
    sketch_directories: list[Path] = []
    # search all the paths one level deep
    for path in directory.iterdir():
        if path.is_dir():
            dir_name = path.name
            if str(dir_name).startswith("."):
                continue
            file_count += 1
            if file_count > _MAX_FILES_SEARCH_LIMIT:
                print(
                    f"More than {_MAX_FILES_SEARCH_LIMIT} files found. Stopping search."
                )
                break

            if looks_like_sketch_directory(path, quick=True):
                sketch_directories.append(path)
            if dir_name.lower() == "examples":
                # Recursively search examples directory for sketch directories
                def search_examples_recursive(
                    examples_path: Path, depth: int = 0, max_depth: int = 3
                ):
                    nonlocal file_count
                    if depth >= max_depth:
                        return
                    for example in examples_path.iterdir():
                        if example.is_dir():
                            if str(example.name).startswith("."):
                                continue
                            file_count += 1
                            if file_count > _MAX_FILES_SEARCH_LIMIT:
                                return
                            if looks_like_sketch_directory(example, quick=True):
                                sketch_directories.append(example)
                            else:
                                # Keep searching deeper if this isn't a sketch directory
                                search_examples_recursive(example, depth + 1, max_depth)

                search_examples_recursive(path)
    # make relative to cwd
    sketch_directories = [p.relative_to(directory) for p in sketch_directories]
    return sketch_directories


def get_sketch_files(directory: Path) -> list[Path]:
    file_count = 0
    files: list[Path] = []
    for root, dirs, filenames in os.walk(directory):
        # ignore hidden directories
        dirs[:] = [d for d in dirs if not d.startswith(".")]
        # ignore fastled_js directory
        dirs[:] = [d for d in dirs if "fastled_js" not in d]
        # ignore hidden files
        filenames = [f for f in filenames if not f.startswith(".")]
        outer_break = False
        for filename in filenames:
            if "platformio.ini" in filename:
                continue
            file_count += 1
            if file_count > _MAX_FILES_SEARCH_LIMIT:
                print(
                    f"More than {_MAX_FILES_SEARCH_LIMIT} files found. Stopping search."
                )
                outer_break = True
                break
            files.append(Path(root) / filename)
        if outer_break:
            break

    return files


def looks_like_fastled_repo(directory: Path = Path(".")) -> bool:
    libprops = directory / "library.properties"
    if not libprops.exists():
        return False
    txt = libprops.read_text(encoding="utf-8", errors="ignore")
    return "FastLED" in txt


def _lots_and_lots_of_files(directory: Path) -> bool:
    return len(get_sketch_files(directory)) > 100


def looks_like_sketch_directory(directory: Path | str | None, quick=False) -> bool:
    if directory is None:
        return False
    directory = Path(directory)
    if looks_like_fastled_repo(directory):
        print("Directory looks like the FastLED repo")
        return False

    if not quick:
        if _lots_and_lots_of_files(directory):
            return False

    # walk the path and if there are over 30 files, return False
    # at the root of the directory there should either be an ino file or a src directory
    # or some cpp files
    # if there is a platformio.ini file, return True
    ino_file_at_root = list(directory.glob("*.ino"))
    if ino_file_at_root:
        return True
    cpp_file_at_root = list(directory.glob("*.cpp"))
    if cpp_file_at_root:
        return True
    platformini_file = list(directory.glob("platformio.ini"))
    if platformini_file:
        return True
    return False


def find_sketch_by_partial_name(
    partial_name: str, search_dir: Path | None = None
) -> Path | None:
    """
    Find a sketch directory by partial name match.

    Args:
        partial_name: Partial name to match against sketch directories
        search_dir: Directory to search in (defaults to current directory)

    Returns:
        Path to the matching sketch directory, or None if no unique match found

    Raises:
        ValueError: If multiple matches are found with no clear best match, or no matches found
    """
    if search_dir is None:
        search_dir = Path(".")

    # First, find all sketch directories
    sketch_directories = find_sketch_directories(search_dir)

    # Normalize the partial name to use forward slashes for cross-platform matching
    partial_name_normalized = partial_name.replace("\\", "/").lower()

    # Get the set of characters in the partial name for similarity check
    partial_chars = set(partial_name_normalized)

    # Find matches where the partial name appears in the path
    matches = []
    for sketch_dir in sketch_directories:
        # Normalize the sketch directory path to use forward slashes
        sketch_str_normalized = str(sketch_dir).replace("\\", "/").lower()

        # Character similarity check: at least 50% of partial name chars must be in target
        target_chars = set(sketch_str_normalized)
        matching_chars = partial_chars & target_chars
        similarity = (
            len(matching_chars) / len(partial_chars) if len(partial_chars) > 0 else 0
        )

        # Check if partial_name matches the directory name or any part of the path
        # AND has sufficient character similarity
        if partial_name_normalized in sketch_str_normalized and similarity >= 0.5:
            matches.append(sketch_dir)

    if len(matches) == 0:
        # Check if this is a total mismatch (low character similarity with all sketches)
        all_low_similarity = True
        for sketch_dir in sketch_directories:
            sketch_str_normalized = str(sketch_dir).replace("\\", "/").lower()
            target_chars = set(sketch_str_normalized)
            matching_chars = partial_chars & target_chars
            similarity = (
                len(matching_chars) / len(partial_chars)
                if len(partial_chars) > 0
                else 0
            )
            if similarity > 0.5:
                all_low_similarity = False
                break

        if all_low_similarity and len(sketch_directories) > 0:
            # List all available sketches
            sketches_str = "\n  ".join(str(s) for s in sketch_directories)
            raise ValueError(
                f"'{partial_name}' does not look like any of the available sketches.\n\n"
                f"Available sketches:\n  {sketches_str}"
            )
        else:
            raise ValueError(f"No sketch directory found matching '{partial_name}'")
    elif len(matches) == 1:
        return matches[0]
    else:
        # Multiple matches - try to find the best match
        # Best match criteria: exact match of the final directory name
        exact_matches = []
        for match in matches:
            # Get the final directory name
            final_dir_name = match.name.lower()
            if final_dir_name == partial_name_normalized:
                exact_matches.append(match)

        if len(exact_matches) == 1:
            # Found exactly one exact match - this is the best match
            return exact_matches[0]
        elif len(exact_matches) > 1:
            # Multiple exact matches - still ambiguous
            matches_str = "\n  ".join(str(m) for m in exact_matches)
            raise ValueError(
                f"Multiple sketch directories found matching '{partial_name}':\n  {matches_str}"
            )
        else:
            # No exact match - ambiguous partial matches
            matches_str = "\n  ".join(str(m) for m in matches)
            raise ValueError(
                f"Multiple sketch directories found matching '{partial_name}':\n  {matches_str}"
            )
