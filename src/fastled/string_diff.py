from pathlib import Path

from fastled._native import is_in_order_match, string_diff

__all__ = ["is_in_order_match", "string_diff", "string_diff_paths"]


def string_diff_paths(
    input_string: str | Path, path_list: list[Path], ignore_case=True
) -> list[tuple[float, Path]]:
    # Normalize path separators to forward slashes for consistent comparison
    string_list = [str(p).replace("\\", "/") for p in path_list]
    input_str = str(input_string).replace("\\", "/")

    tmp = string_diff(input_str, string_list, ignore_case)
    out: list[tuple[float, Path]] = []
    for i, j in tmp:
        for orig_path in path_list:
            if str(orig_path).replace("\\", "/") == j:
                out.append((i, orig_path))
                break
    return out
