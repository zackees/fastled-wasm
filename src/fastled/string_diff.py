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

    is_substring = False
    for s in string_list:
        if input_string in s:
            is_substring = True
            break

    if is_substring:
        string_list = [s for s in string_list if input_string in s]

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
