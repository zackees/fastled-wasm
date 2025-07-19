from pathlib import Path

from rapidfuzz import fuzz

from fastled.string_diff import string_diff_paths


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
        print("\nMultiple Directories found, choose one:")
        for i, sketch_dir in enumerate(sketch_directories):
            print(f"  [{i+1}]: {sketch_dir}")
        which = input(
            "\nPlease specify a sketch directory\nYou can enter a number or type a fuzzy search: "
        ).strip()
        try:
            index = int(which) - 1
            return str(sketch_directories[index])
        except (ValueError, IndexError):
            inputs = [p for p in sketch_directories]

            if is_followup:
                # On follow-up, find the closest match by fuzzy distance
                distances = []
                for path in inputs:
                    path_str = str(path).replace("\\", "/")
                    dist = fuzz.token_sort_ratio(which.lower(), path_str.lower())
                    distances.append((dist, path))

                # Get the best distance and return the closest match(es)
                best_distance = max(distances, key=lambda x: x[0])[0]
                best_matches = [
                    path for dist, path in distances if dist == best_distance
                ]

                if len(best_matches) == 1:
                    example = best_matches[0]
                    return str(example)
                else:
                    # If still multiple matches with same distance, recurse again
                    return select_sketch_directory(
                        best_matches, cwd_is_fastled, is_followup=True
                    )
            else:
                # First call - use original fuzzy matching (allows ambiguity)
                top_hits: list[tuple[float, Path]] = string_diff_paths(which, inputs)
                if len(top_hits) == 1:
                    example = top_hits[0][1]
                    return str(example)
                else:
                    # Recursive call with is_followup=True for more precise matching
                    return select_sketch_directory(
                        [p for _, p in top_hits], cwd_is_fastled, is_followup=True
                    )
    return None
