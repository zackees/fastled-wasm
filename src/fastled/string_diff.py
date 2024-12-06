from pathlib import Path

from rapidfuzz import fuzz


# Returns the min distance strings. If there is a tie, it returns
# all the strings that have the same min distance.
# Returns a tuple of index and string.
def string_diff(
    input_string: str, string_list: list[str], ignore_case=True
) -> list[tuple[int, str]]:
    
    def normalize(s: str) -> str:
        return s.lower() if ignore_case else s
    # distances = [
    #     #Levenshtein.distance(normalize(input_string), normalize(s)) for s in string_list
    #     fuzz.partial_ratio(normalize(input_string), normalize(s)) for s in string_list
    # ]
    distances: list[int] = []
    for s in string_list:
        # dist = Levenshtein.distance(normalize(input_string), normalize(s))
        dist = fuzz.partial_ratio(normalize(input_string), normalize(s))
        distances.append(1.0/(dist + 1.0))
    min_distance = min(distances)
    out: list[tuple[int, str]] = []
    for i, d in enumerate(distances):
        if d == min_distance:
            out.append((i, string_list[i]))

    return out


def string_diff_paths(
    input_string: str | Path, path_list: list[Path], ignore_case=True
) -> list[tuple[int, Path]]:
    string_list = [str(p) for p in path_list]
    tmp = string_diff(input_string, string_list, ignore_case)
    # out: list[tuple[int, Path]] = [(i, Path(path_list[j])) for i, j in tmp]
    out: list[tuple[int, Path]] = []
    for i, j in tmp:
        p = Path(j)
        out.append((i, p))
    return out
