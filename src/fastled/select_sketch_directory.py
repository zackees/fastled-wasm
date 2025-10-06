from pathlib import Path
from typing import Callable, TypeVar, Union

from fastled.string_diff import string_diff

T = TypeVar("T")


def _disambiguate_user_choice(
    options: list[T],
    option_to_str: Callable[[T], str] = str,
    prompt: str = "Multiple matches found. Please choose:",
    default_index: int = 0,
) -> Union[T, None]:
    """
    Present multiple options to the user with a default selection.

    Args:
        options: List of options to choose from
        option_to_str: Function to convert option to display string
        prompt: Prompt message to show user
        default_index: Index of the default option (0-based)

    Returns:
        Selected option or None if cancelled
    """
    if not options:
        return None

    if len(options) == 1:
        return options[0]

    # Ensure default_index is valid
    if default_index < 0 or default_index >= len(options):
        default_index = 0

    print(f"\n{prompt}")
    for i, option in enumerate(options):
        option_str = option_to_str(option)
        if i == default_index:
            print(f"  [{i+1}]: [{option_str}]")  # Default option shown in brackets
        else:
            print(f"  [{i+1}]: {option_str}")

    default_option_str = option_to_str(options[default_index])
    user_input = input(
        f"\nEnter number or name (default: [{default_option_str}]): "
    ).strip()

    # Handle empty input - select default
    if not user_input:
        return options[default_index]

    # Try to parse as number
    try:
        index = int(user_input) - 1
        if 0 <= index < len(options):
            return options[index]
    except ValueError:
        pass

    # Try to match by name (case insensitive)
    user_input_lower = user_input.lower()
    for option in options:
        option_str = option_to_str(option).lower()
        if option_str == user_input_lower:
            return option

    # Try partial match
    matches = []
    for option in options:
        option_str = option_to_str(option)
        if user_input_lower in option_str.lower():
            matches.append(option)

    if len(matches) == 1:
        return matches[0]
    elif len(matches) > 1:
        # Recursive disambiguation with the filtered matches
        return _disambiguate_user_choice(
            matches,
            option_to_str,
            f"Multiple partial matches for '{user_input}':",
            0,  # Reset default to first match
        )

    # Try fuzzy matching as fallback
    # For better fuzzy matching on paths, extract just the last component (basename)
    # to avoid the "examples/" prefix interfering with matching
    from pathlib import Path as PathLib

    option_basenames = []
    for option in options:
        option_str = option_to_str(option)
        # Extract basename for fuzzy matching
        basename = (
            PathLib(option_str).name
            if "/" in option_str or "\\" in option_str
            else option_str
        )
        option_basenames.append(basename)

    fuzzy_results = string_diff(user_input, option_basenames)

    if fuzzy_results:
        # Map fuzzy results back to original options
        fuzzy_matches = []
        for _, matched_basename in fuzzy_results:
            for i, basename in enumerate(option_basenames):
                if basename == matched_basename:
                    fuzzy_matches.append(options[i])
                    break

        if len(fuzzy_matches) == 1:
            return fuzzy_matches[0]
        elif len(fuzzy_matches) > 1:
            # Recursive disambiguation with fuzzy matches
            return _disambiguate_user_choice(
                fuzzy_matches,
                option_to_str,
                f"Multiple fuzzy matches for '{user_input}':",
                0,
            )

    # No match found
    print(f"No match found for '{user_input}'. Please try again.")
    return _disambiguate_user_choice(options, option_to_str, prompt, default_index)


def select_sketch_directory(
    sketch_directories: list[Path], cwd_is_fastled: bool, is_followup: bool = False
) -> str | None:
    if cwd_is_fastled:
        exclude = ["src", "dev", "tests"]
        for ex in exclude:
            p = Path(ex)
            if p in sketch_directories:
                sketch_directories.remove(p)

    if len(sketch_directories) == 1:
        print(f"\nUsing sketch directory: {sketch_directories[0]}")
        return str(sketch_directories[0])
    elif len(sketch_directories) > 1:
        # First scan with >4 directories: return None (too many to auto-select)
        if not is_followup and len(sketch_directories) > 4:
            return None

        # Prompt user to disambiguate
        result = _disambiguate_user_choice(
            sketch_directories,
            option_to_str=lambda x: str(x),
            prompt="Multiple Directories found, choose one:",
            default_index=0,
        )

        if result is None:
            return None

        return str(result)
    return None
