import subprocess
import time
from pathlib import Path
from typing import Callable

from compile_lock import COMPILE_LOCK

TIME_START = time.time()


def _sync_src_to_target(
    src: Path, dst: Path, callback: Callable[[], None] | None = None
) -> bool:
    """Sync the volume mapped source directory to the FastLED source directory."""
    suppress_print = (
        TIME_START + 30 > time.time()
    )  # Don't print during initial volume map.
    if not src.exists():
        # Volume is not mapped in so we don't rsync it.
        print(f"Skipping rsync, as fastled src at {src} doesn't exist")
        return False
    try:
        exclude_hidden = "--exclude=.*/"  # suppresses folders like .mypy_cache/
        print("\nSyncing source directories...")
        with COMPILE_LOCK:
            # Use rsync to copy files, preserving timestamps and deleting removed files
            cp: subprocess.CompletedProcess = subprocess.run(
                [
                    "rsync",
                    "-av",
                    "--info=NAME",
                    "--delete",
                    f"{src}/",
                    f"{dst}/",
                    exclude_hidden,
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            if cp.returncode == 0:
                changed = False
                changed_lines: list[str] = []
                lines = cp.stdout.split("\n")
                for line in lines:
                    suffix = line.strip().split(".")[-1]
                    if suffix in ["cpp", "h", "hpp", "ino", "py", "js", "html", "css"]:
                        if not suppress_print:
                            print(f"Changed file: {line}")
                        changed = True
                        changed_lines.append(line)
                if changed:
                    if not suppress_print:
                        print(f"FastLED code had updates: {changed_lines}")
                    if callback:
                        callback()
                    return True
                print("Source directory synced successfully with no changes")
                return False
            else:
                print(f"Error syncing directories: {cp.stdout}\n\n{cp.stderr}")
                return False

    except subprocess.CalledProcessError as e:
        print(f"Error syncing directories: {e.stdout}\n\n{e.stderr}")
    except Exception as e:
        print(f"Error syncing directories: {e}")
    return False


class CodeSync:

    def __init__(self, volume_mapped_src: Path, rsync_dest: Path):
        self.volume_mapped_src = volume_mapped_src
        self.rsync_dest = rsync_dest

    def sync_src_to_target(
        self,
        volume_mapped_src: Path | None = None,
        rsync_dest: Path | None = None,
        callback: Callable[[], None] | None = None,
    ) -> bool:
        """Sync the volume mapped source directory to the FastLED source directory."""

        if volume_mapped_src is None or rsync_dest is None:
            assert (
                volume_mapped_src is None and rsync_dest is None
            ), f"Both must be None: {volume_mapped_src} {rsync_dest}"
        volume_mapped_src = volume_mapped_src or self.volume_mapped_src
        rsync_dest = rsync_dest or self.rsync_dest
        return _sync_src_to_target(
            self.volume_mapped_src, self.rsync_dest, callback=callback
        )

    def sync_source_directory_if_volume_is_mapped(
        self,
        callback: Callable[[], None] | None = None,
    ) -> bool:
        """Sync the volume mapped source directory to the FastLED source directory."""
        if not self.volume_mapped_src.exists():
            # Volume is not mapped in so we don't rsync it.
            print("Skipping rsync, as fastled src volume not mapped")
            return False
        print("Syncing source directories because host is mapped in")
        out: bool = self.sync_src_to_target(callback=callback)
        return out
