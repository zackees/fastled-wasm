from pathlib import Path

from fastled.string_diff import string_diff_paths


def select_sketch_directory(
    sketch_directories: list[Path], cwd_is_fastled: bool
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
        which = input("\nPlease specify a sketch directory: ").strip()
        try:
            index = int(which) - 1
            return str(sketch_directories[index])
        except (ValueError, IndexError):
            inputs = [p for p in sketch_directories]
            top_hits: list[tuple[float, Path]] = string_diff_paths(which, inputs)
            if len(top_hits) == 1:
                example = top_hits[0][1]
                return str(example)
            else:
                return select_sketch_directory([p for _, p in top_hits], cwd_is_fastled)
    return None
