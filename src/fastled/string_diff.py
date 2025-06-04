from pathlib import Path

from rapidfuzz import fuzz


def _filter_out_obvious_bad_choices(
    input_str: str, string_list: list[str]
) -> list[str]:
    """
    Filter out strings that are too different from the input string.
    This is a heuristic and may not be perfect.
    """
    input_chars = set(input_str)
    filtered_list = []
    for s in string_list:
        # Check if at least half of the input characters are in the string
        s_chars = set(s)
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
    input_chars = [c for c in input_str if c != " "]
    other_chars = list(other)
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

    map_string: dict[str, str] = {}

    if ignore_case:
        map_string = {s.lower(): s for s in string_list}
    else:
        map_string = {s: s for s in string_list}

    if ignore_case:
        string_list = [s.lower() for s in string_list]
        input_string = input_string.lower()

    # Apply set membership filtering for queries with 3+ characters
    if len(input_string) >= 3:
        string_list = _filter_out_obvious_bad_choices(input_string, string_list)

    # Second filter: exact substring filtering if applicable
    is_substring = False
    for s in string_list:
        if input_string in s:
            is_substring = True
            break

    if is_substring:
        string_list = [s for s in string_list if input_string in s]

    # Third filter: in order exact match filtering if applicable.
    is_in_order = False
    for s in string_list:
        if is_in_order_match(input_string, s):
            is_in_order = True
            break

    if is_in_order:
        string_list = [s for s in string_list if is_in_order_match(input_string, s)]

    distances: list[float] = []
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
    string_list = [str(p) for p in path_list]
    tmp = string_diff(str(input_string), string_list, ignore_case)
    out: list[tuple[float, Path]] = []
    for i, j in tmp:
        p = Path(j)
        out.append((i, p))
    return out
