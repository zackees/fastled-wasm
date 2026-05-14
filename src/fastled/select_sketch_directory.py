"""Compatibility prompt helpers backed by native Rust matching logic.

Python owns the interactive prompt loop for existing callers. Sketch selection
preparation and choice resolution are delegated to ``fastled._native``.
"""

from pathlib import Path
from typing import Callable, TypeVar, Union

from fastled._native import prepare_sketch_selection as _native_prepare_sketch_selection
from fastled._native import resolve_prompt_choice as _native_resolve_prompt_choice

T = TypeVar("T")


def _find_matching_option(
    options: list[T], option_to_str: Callable[[T], str], selected: str
) -> T | None:
    selected_lower = selected.lower()
    for option in options:
        if option_to_str(option).lower() == selected_lower:
            return option
    return None


def _filter_options_by_labels(
    options: list[T], option_to_str: Callable[[T], str], labels: list[str]
) -> list[T]:
    remaining = list(options)
    narrowed: list[T] = []
    for label in labels:
        match = _find_matching_option(remaining, option_to_str, label)
        if match is None:
            continue
        narrowed.append(match)
        remaining.remove(match)
    return narrowed


def _disambiguate_user_choice(
    options: list[T],
    option_to_str: Callable[[T], str] = str,
    prompt: str = "Multiple matches found. Please choose:",
    default_index: int = 0,
) -> Union[T, None]:
    """
    Present multiple options to the user with a default selection.

    Matching and narrowing are delegated to the native Rust implementation;
    Python only preserves the compatibility prompt surface for existing callers.
    """
    if not options:
        return None

    if len(options) == 1:
        return options[0]

    current_options = list(options)
    current_prompt = prompt
    current_default = default_index

    while True:
        if current_default < 0 or current_default >= len(current_options):
            current_default = 0

        print(f"\n{current_prompt}")
        for i, option in enumerate(current_options):
            option_str = option_to_str(option)
            if i == current_default:
                print(f"  [{i + 1}]: [{option_str}]")
            else:
                print(f"  [{i + 1}]: {option_str}")

        default_option_str = option_to_str(current_options[current_default])
        user_input = input(
            f"\nEnter number or name (default: [{default_option_str}]): "
        ).strip()

        option_labels = [option_to_str(option) for option in current_options]
        status, selected, narrowed = _native_resolve_prompt_choice(
            user_input,
            option_labels,
            current_default,
        )

        if status == "selected" and selected is not None:
            return _find_matching_option(current_options, option_to_str, selected)

        if status == "narrowed":
            narrowed_options = _filter_options_by_labels(
                current_options,
                option_to_str,
                list(narrowed),
            )
            if not narrowed_options:
                print(f"No match found for '{user_input}'. Please try again.")
                continue

            user_input_lower = user_input.lower()
            is_partial = (
                sum(1 for label in option_labels if user_input_lower in label.lower())
                > 1
            )
            current_prompt = (
                f"Multiple partial matches for '{user_input}':"
                if is_partial
                else f"Multiple fuzzy matches for '{user_input}':"
            )
            current_options = narrowed_options
            current_default = 0
            continue

        print(f"No match found for '{user_input}'. Please try again.")


def select_sketch_directory(
    sketch_directories: list[Path], cwd_is_fastled: bool, is_followup: bool = False
) -> str | None:
    status, selected, options = _native_prepare_sketch_selection(
        [str(path) for path in sketch_directories],
        cwd_is_fastled,
        is_followup,
    )

    if status == "selected" and selected is not None:
        print(f"\nUsing sketch directory: {selected}")
        return str(selected)

    if status != "prompt":
        return None

    result = _disambiguate_user_choice(
        [Path(option) for option in options],
        option_to_str=lambda path: str(path),
        prompt="Multiple Directories found, choose one:",
        default_index=0,
    )
    return str(result) if result is not None else None
