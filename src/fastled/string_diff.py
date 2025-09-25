from pathlib import Path

from rapidfuzz import fuzz


def _filter_out_obvious_bad_choices(
    input_str: str, string_list: list[str]
) -> list[str]:
    """
    Filter out strings that are too different from the input string.
    This is a heuristic and may not be perfect.
    """
    if not input_str.strip():  # Handle empty input
        return string_list

    input_chars = set(input_str.lower())
    filtered_list = []
    for s in string_list:
        # Check if at least half of the input characters are in the string
        s_chars = set(s.lower())
        common_chars = input_chars.intersection(s_chars)
        if len(common_chars) >= len(input_chars) / 2:
            filtered_list.append(s)
    return filtered_list


def is_in_order_match(input_str: str, other: str) -> bool:
    """
    Check if the input string is an in-order match for any string in the list.
    An in-order match means that the characters of the input string appear
    in the same order in the string from the list, ignoring spaces in the input.
    """

    # Remove spaces from input string for matching
    input_chars = [c.lower() for c in input_str if c != " "]
    other_chars = [c.lower() for c in other]
    input_index = 0
    other_index = 0
    while input_index < len(input_chars) and other_index < len(other_chars):
        if input_chars[input_index] == other_chars[other_index]:
            input_index += 1
        other_index += 1
    # If we reached the end of the input string, it means all characters were found in order
    if input_index == len(input_chars):
        return True
    return False


# Returns the min distance strings. If there is a tie, it returns
# all the strings that have the same min distance.
# Returns a tuple of index and string.
def string_diff(
    input_string: str, string_list: list[str], ignore_case=True
) -> list[tuple[float, str]]:

    def normalize(s: str) -> str:
        return s.lower() if ignore_case else s

    # Handle empty input or empty list
    if not input_string.strip():
        # Return all strings with equal distance for empty input
        return [(i, s) for i, s in enumerate(string_list)]

    if not string_list:
        return []

    map_string: dict[str, str] = {}

    if ignore_case:
        map_string = {s.lower(): s for s in string_list}
    else:
        map_string = {s: s for s in string_list}

    original_string_list = string_list.copy()
    if ignore_case:
        string_list = [s.lower() for s in string_list]
        input_string = input_string.lower()

    # Check for exact matches, but also check if there are other substring matches
    exact_matches = [s for s in string_list if s == input_string]
    substring_matches = [s for s in string_list if input_string in s]

    # If there's exactly one exact match, and there are substring matches,
    # check if we should prioritize the exact match or return all variants
    if len(exact_matches) == 1 and len(substring_matches) > 1:
        exact_match = exact_matches[0]
        other_substring_matches = [s for s in substring_matches if s != exact_match]

        # Prioritize exact match only if it appears at the start of other matches
        # AND those matches have a camelCase boundary (indicating compound words)
        # We need to use the original (non-lowercased) strings for camelCase detection
        should_prioritize_exact = True
        original_exact_match = map_string[exact_match]  # Get the original casing

        for other_match in other_substring_matches:
            original_other_match = map_string[other_match]  # Get the original casing

            if not original_other_match.lower().startswith(
                original_exact_match.lower()
            ):
                # If the exact match isn't at the start, don't prioritize
                should_prioritize_exact = False
                break

            # Check for camelCase boundary after the exact match in the ORIGINAL string
            remainder = original_other_match[len(original_exact_match) :]
            if remainder and remainder[0].isupper():
                # Only prioritize exact match if the exact match is very short (4 chars or less)
                # AND the remainder suggests a different concept
                if len(original_exact_match) <= 4 and len(remainder) >= 6:
                    # This looks like a camelCase compound word (e.g., "wasm" -> "WasmScreenCoords")
                    continue
                else:
                    # This looks like a variant (e.g., "Noise" -> "NoisePlayground", "Fire2012" -> "Fire2012WithPalette")
                    should_prioritize_exact = False
                    break
            else:
                # This looks like a variant/extension (e.g., "Blur" -> "Blur2d")
                should_prioritize_exact = False
                break

        if should_prioritize_exact:
            out: list[tuple[float, str]] = []
            for i, s in enumerate(exact_matches):
                s_mapped = map_string.get(s, s)
                out.append((i, s_mapped))
            return out
        else:
            # Apply character count filtering only for very specific compound terms
            # Main criteria: contains numbers AND ends with numbers/letters (like Wave2d, Fire2012)
            original_exact_match = map_string[exact_match]
            should_apply_char_filter = (
                len(original_exact_match) >= 5  # Longer terms
                and any(c.isdigit() for c in original_exact_match)  # Contains numbers
                and (
                    original_exact_match[-1].isdigit()
                    or original_exact_match[-1].islower()
                )  # Ends specifically (compound pattern)
            )

            if should_apply_char_filter:
                # Filter substring matches based on extra character count
                # Use a more lenient threshold for shorter base terms
                if len(original_exact_match) <= 6:
                    # For short terms, allow more extra chars (e.g., "Wave2d" + "FxWave2d")
                    MAX_EXTRA_CHARS = min(10, len(original_exact_match) * 2)
                else:
                    # For longer terms, allow significant extensions (e.g., "Fire2012" + "Fire2012WithPalette")
                    MAX_EXTRA_CHARS = 12

                filtered_matches = []

                for s in substring_matches:
                    original_s = map_string[s]
                    if s == exact_match:
                        # Always include the exact match
                        filtered_matches.append(s)
                    else:
                        # Calculate extra characters
                        extra_chars = len(original_s) - len(original_exact_match)
                        if extra_chars <= MAX_EXTRA_CHARS:
                            filtered_matches.append(s)

                # Return filtered matches
                out: list[tuple[float, str]] = []
                for i, s in enumerate(filtered_matches):
                    s_mapped = map_string.get(s, s) or s
                    out.append((i, s_mapped))
                return out
            else:
                # Return all substring matches (original behavior for base terms)
                out: list[tuple[float, str]] = []
                for i, s in enumerate(substring_matches):
                    s_mapped = map_string.get(s, s) or s
                    out.append((i, s_mapped))
                return out

    # If there's only an exact match and no other substring matches, return just the exact match
    if exact_matches and len(substring_matches) == 1:
        out: list[tuple[float, str]] = []
        for i, s in enumerate(exact_matches):
            s_mapped = map_string.get(s, s)
            out.append((i, s_mapped))
        return out

    # Apply set membership filtering for queries with 3+ characters
    if len(input_string.strip()) >= 3:
        filtered = _filter_out_obvious_bad_choices(input_string, string_list)
        if filtered:  # Only apply filter if it doesn't eliminate everything
            string_list = filtered

    # Second filter: exact substring filtering if applicable
    if substring_matches:
        string_list = substring_matches
        # Return all substring matches
        out: list[tuple[float, str]] = []
        for i, s in enumerate(string_list):
            s_mapped = map_string.get(s, s)
            out.append((i, s_mapped))
        return out

    # Third filter: in order exact match filtering if applicable.
    in_order_matches = [s for s in string_list if is_in_order_match(input_string, s)]
    if in_order_matches:
        string_list = in_order_matches

    # Calculate distances
    distances: list[float] = []
    for s in string_list:
        dist = fuzz.token_sort_ratio(normalize(input_string), normalize(s))
        distances.append(1.0 / (dist + 1.0))

    # Handle case where no strings remain after filtering
    if not distances:
        # Fall back to original list and calculate distances
        string_list = original_string_list.copy()
        if ignore_case:
            string_list = [s.lower() for s in string_list]

        distances = []
        for s in string_list:
            dist = fuzz.token_sort_ratio(normalize(input_string), normalize(s))
            distances.append(1.0 / (dist + 1.0))

    min_distance = min(distances)
    out: list[tuple[float, str]] = []
    for i, d in enumerate(distances):
        if d == min_distance:
            s = string_list[i]
            s_mapped = map_string.get(s, s)
            out.append((i, s_mapped))

    return out


def string_diff_paths(
    input_string: str | Path, path_list: list[Path], ignore_case=True
) -> list[tuple[float, Path]]:
    # Normalize path separators to forward slashes for consistent comparison
    string_list = [str(p).replace("\\", "/") for p in path_list]
    input_str = str(input_string).replace("\\", "/")

    tmp = string_diff(input_str, string_list, ignore_case)
    out: list[tuple[float, Path]] = []
    for i, j in tmp:
        # Find the original path that matches the normalized result
        for idx, orig_path in enumerate(path_list):
            if str(orig_path).replace("\\", "/") == j:
                out.append((i, orig_path))
                break
    return out
