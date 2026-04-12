from __future__ import annotations

import os
import platform
from dataclasses import dataclass
from importlib import resources
from pathlib import Path

DEFAULT_FASTLED_PREFIX = "fastledsource"
DEFAULT_SKETCH_PREFIX = "sketchsource"
DEFAULT_DWARF_PREFIX = "dwarfsource"


def _git_bash_prefixes() -> tuple[str, ...]:
    try:
        git_bash_root = resources.files("zcmds_win32").joinpath("git-bash-bin")
    except ModuleNotFoundError:
        return ()

    with resources.as_file(git_bash_root) as resolved_root:
        return _path_prefix_variants(resolved_root)


def _path_prefix_variants(path: Path) -> tuple[str, ...]:
    as_posix = path.as_posix().rstrip("/")
    drive = path.drive.rstrip(":")
    prefixes = [as_posix]

    if drive:
        drive_lower = drive.lower()
        prefixes.extend(
            [
                as_posix.replace(f"{drive}:", f"/{drive_lower}", 1),
                as_posix.replace(f"{drive}:", f"/{drive}:", 1),
            ]
        )

    return tuple(dict.fromkeys(prefixes))


def normalize_windows_path(path_str: str) -> str:
    if platform.system() != "Windows":
        return path_str

    for prefix in _git_bash_prefixes():
        prefix = prefix.rstrip("/") + "/"
        if path_str.startswith(prefix):
            relative_path = path_str[len(prefix) :]
            if not relative_path.startswith("/"):
                relative_path = "/" + relative_path
            return relative_path
    return path_str


@dataclass(frozen=True)
class DebugSymbolConfig:
    sketch_dir: Path
    fastled_dir: Path | None = None
    emsdk_path: Path | None = None
    fastled_prefix: str = DEFAULT_FASTLED_PREFIX
    sketch_prefix: str = DEFAULT_SKETCH_PREFIX
    dwarf_prefix: str = DEFAULT_DWARF_PREFIX

    @property
    def source_roots(self) -> list[tuple[str, Path]]:
        roots = [(self.sketch_prefix, self.sketch_dir.resolve())]
        if self.fastled_dir is not None:
            roots.append((self.fastled_prefix, self.fastled_dir.resolve() / "src"))
            roots.append(("headers", self.fastled_dir.resolve() / "src"))
        if self.emsdk_path is not None:
            roots.append(("emsdk", self.emsdk_path.resolve()))
        return roots


def load_debug_symbol_config(
    sketch_dir: Path,
    fastled_dir: Path | None = None,
    emsdk_path: Path | None = None,
) -> DebugSymbolConfig:
    config = DebugSymbolConfig(
        sketch_dir=sketch_dir,
        fastled_dir=fastled_dir,
        emsdk_path=emsdk_path,
    )

    build_flags = None
    if fastled_dir is not None:
        candidate = (
            fastled_dir / "src" / "platforms" / "wasm" / "compiler" / "build_flags.toml"
        )
        if candidate.exists():
            build_flags = candidate

    if build_flags is None:
        return config

    with open(build_flags, "rb") as handle:
        import tomllib

        data = tomllib.load(handle)

    dwarf = data.get("dwarf", {})
    return DebugSymbolConfig(
        sketch_dir=sketch_dir,
        fastled_dir=fastled_dir,
        emsdk_path=emsdk_path,
        fastled_prefix=dwarf.get("fastled_prefix", DEFAULT_FASTLED_PREFIX),
        sketch_prefix=dwarf.get("sketch_prefix", DEFAULT_SKETCH_PREFIX),
        dwarf_prefix=dwarf.get("dwarf_prefix", DEFAULT_DWARF_PREFIX),
    )


class DebugSymbolResolver:
    def __init__(self, config: DebugSymbolConfig):
        self.config = config

    @property
    def prefixes(self) -> tuple[str, str, str]:
        return (
            self.config.fastled_prefix,
            self.config.sketch_prefix,
            self.config.dwarf_prefix,
        )

    def prune_path(self, request_path: str) -> str | None:
        normalized = normalize_windows_path(request_path.strip()).replace("\\", "/")
        normalized = normalized.lstrip("/")
        parts = Path(normalized).parts
        current_prefixes = set(self.prefixes)

        for prefix in self.prefixes:
            if normalized == prefix or normalized.startswith(f"{prefix}/"):
                return normalized

        prefix_index = None
        prefix_value = None
        for index, part in enumerate(parts):
            if part in current_prefixes:
                prefix_index = index
                prefix_value = part

        if prefix_index is None or prefix_value is None:
            return None

        result = "/".join(parts[prefix_index:]).lstrip("/")
        if result.startswith("C:/") or result.startswith("C:\\"):
            marker = "fastled/src/"
            result = result.replace("\\", "/")
            if marker in result:
                result = result[result.index(marker) + len(marker) :]
                return f"{self.config.fastled_prefix}/{result}"
        return result

    def resolve(self, request_path: str, check_exists: bool = True) -> Path:
        if ".." in request_path.replace("\\", "/").split("/"):
            raise ValueError(f"Invalid path: {request_path}")

        pruned = self.prune_path(request_path)
        if not pruned:
            raise ValueError(f"Invalid path: {request_path}")

        normalized = pruned.replace("\\", "/").lstrip("/")
        if normalized.startswith(f"{self.config.dwarf_prefix}/"):
            normalized = normalized[len(self.config.dwarf_prefix) + 1 :]
        elif normalized == self.config.dwarf_prefix:
            raise ValueError(f"Invalid path: {request_path}")

        for prefix, root in self.config.source_roots:
            if normalized == prefix:
                target = root
            elif normalized.startswith(f"{prefix}/"):
                suffix = normalized[len(prefix) + 1 :]
                target = root / suffix
            else:
                continue

            resolved_target = target.resolve(strict=False)
            resolved_root = root.resolve()
            if not _is_within(resolved_target, resolved_root):
                raise ValueError(f"Invalid path: {request_path}")
            if check_exists and not resolved_target.exists():
                raise FileNotFoundError(f"Could not find path {resolved_target}")
            return resolved_target

        fastled_root = (
            self.config.fastled_dir.resolve() / "src"
            if self.config.fastled_dir
            else None
        )
        if fastled_root is not None:
            resolved_target = (fastled_root / normalized).resolve(strict=False)
            if _is_within(resolved_target, fastled_root):
                if check_exists and not resolved_target.exists():
                    raise FileNotFoundError(f"Could not find path {resolved_target}")
                return resolved_target

        raise ValueError(f"Invalid path: {request_path}")


def _is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def guess_emsdk_path() -> Path | None:
    emsdk_env = os.environ.get("EMSDK")
    if emsdk_env:
        return Path(emsdk_env)
    return None
