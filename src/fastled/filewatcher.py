"""File system watcher implementation using watchdog"""

import hashlib
import os
import queue
import threading
import time
from contextlib import redirect_stdout
from multiprocessing import Process, Queue
from pathlib import Path
from queue import Empty
from typing import Dict, Set

from watchdog.events import FileSystemEvent, FileSystemEventHandler
from watchdog.observers import Observer
from watchdog.observers.api import BaseObserver

from fastled.settings import FILE_CHANGED_DEBOUNCE_SECONDS

_WATCHER_TIMEOUT = 0.1


def file_watcher_enabled() -> bool:
    """Check if watchdog is disabled"""
    return os.getenv("NO_FILE_WATCHING", "0") == "1"


def file_watcher_set(enabled: bool) -> None:
    """Set the file watcher enabled state"""
    os.environ["NO_FILE_WATCHING"] = "1" if not enabled else "0"


class MyEventHandler(FileSystemEventHandler):
    def __init__(
        self,
        change_queue: queue.Queue,
        excluded_patterns: Set[str],
        file_hashes: Dict[str, str],
    ) -> None:
        super().__init__()
        self.change_queue = change_queue
        self.excluded_patterns = excluded_patterns
        self.file_hashes = file_hashes

    def _get_file_hash(self, filepath: str) -> str:
        try:
            with open(filepath, "rb") as f:
                return hashlib.md5(f.read()).hexdigest()
        except Exception:  # pylint: disable=broad-except
            return ""

    def on_modified(self, event: FileSystemEvent) -> None:
        if not event.is_directory:
            # Convert src_path to str if it's bytes
            src_path = (
                event.src_path.decode()
                if isinstance(event.src_path, bytes)
                else event.src_path
            )
            path = Path(src_path)
            # Check if any part of the path matches excluded patterns
            if not any(part in self.excluded_patterns for part in path.parts):
                new_hash = self._get_file_hash(src_path)
                if new_hash and new_hash != self.file_hashes.get(src_path):
                    self.file_hashes[src_path] = new_hash
                    self.change_queue.put(src_path)


class FileChangedNotifier(threading.Thread):
    """Watches a directory for file changes and queues notifications"""

    def __init__(
        self,
        path: str,
        debounce_seconds: float = FILE_CHANGED_DEBOUNCE_SECONDS,
        excluded_patterns: list[str] | None = None,
    ) -> None:
        """Initialize the notifier with a path to watch

        Args:
            path: Directory path to watch for changes
            debounce_seconds: Minimum time between notifications for the same file
            excluded_patterns: List of directory/file patterns to exclude from watching
        """
        super().__init__(daemon=True)
        self.path = path
        self.observer: BaseObserver | None = None
        self.event_handler: MyEventHandler | None = None

        # Combine default and user-provided patterns
        self.excluded_patterns = (
            set(excluded_patterns) if excluded_patterns is not None else set()
        )
        self.stopped = False
        self.change_queue: queue.Queue = queue.Queue()
        self.last_notification: Dict[str, float] = {}
        self.file_hashes: Dict[str, str] = {}
        self.debounce_seconds = debounce_seconds

    def stop(self) -> None:
        """Stop watching for changes"""
        print("watcher stop")
        self.stopped = True
        if self.observer:
            self.observer.stop()
            self.observer.join()
            self.observer = None
            self.event_handler = None

    def run(self) -> None:
        """Thread main loop - starts watching for changes"""
        self.event_handler = MyEventHandler(
            self.change_queue, self.excluded_patterns, self.file_hashes
        )
        self.observer = Observer()
        self.observer.schedule(self.event_handler, self.path, recursive=True)
        self.observer.start()

        try:
            while not self.stopped:
                time.sleep(0.1)
        except KeyboardInterrupt:
            print("File watcher stopped by user.")
        finally:
            self.stop()

    def get_next_change(self, timeout: float = _WATCHER_TIMEOUT) -> str | None:
        """Get the next file change event from the queue

        Args:
            timeout: How long to wait for next change in seconds

        Returns:
            Changed filepath or None if no change within timeout
        """
        if file_watcher_enabled():
            time.sleep(timeout)
            return None
        try:
            filepath = self.change_queue.get(timeout=timeout)
            current_time = time.time()

            # Check if we've seen this file recently
            last_time = self.last_notification.get(filepath, 0)
            if current_time - last_time < self.debounce_seconds:
                return None

            self.last_notification[filepath] = current_time
            return filepath
        except KeyboardInterrupt:
            raise
        except queue.Empty:
            return None

    def get_all_changes(self, timeout: float = _WATCHER_TIMEOUT) -> list[str]:
        """Get all file change events from the queue

        Args:
            timeout: How long to wait for next change in seconds

        Returns:
            List of changed filepaths
        """
        changed_files = []
        while True:
            changed_file = self.get_next_change(timeout=timeout)
            if changed_file is None:
                break
            changed_files.append(changed_file)
        # clear all the changes from the queue
        self.change_queue.queue.clear()
        return changed_files


def _process_wrapper(root: Path, excluded_patterns: list[str], queue: Queue):
    with open(os.devnull, "w") as fnull:  # Redirect to /dev/null
        with redirect_stdout(fnull):
            watcher = FileChangedNotifier(
                str(root), excluded_patterns=excluded_patterns
            )
            watcher.start()
            while True:
                try:
                    changed_files = watcher.get_all_changes()
                    for file in changed_files:
                        queue.put(file)
                except KeyboardInterrupt:
                    break
            watcher.stop()


class ProcessWraperTask:
    def __init__(self, root: Path, excluded_patterns: list[str], queue: Queue) -> None:
        self.root = root
        self.excluded_patterns = excluded_patterns
        self.queue = queue

    def run(self):
        _process_wrapper(self.root, self.excluded_patterns, self.queue)


class FileWatcherProcess:
    def __init__(self, root: Path, excluded_patterns: list[str]) -> None:
        self.queue: Queue = Queue()
        task = ProcessWraperTask(root, excluded_patterns, self.queue)
        self.process = Process(
            target=task.run,
            daemon=True,
        )
        self.process.start()
        self.global_debounce = FILE_CHANGED_DEBOUNCE_SECONDS

    def stop(self):
        self.process.terminate()
        self.process.join()
        self.queue.close()
        self.queue.join_thread()

    def get_all_changes(self, timeout: float | None = None) -> list[str]:
        changed_files = []
        block = timeout is not None

        while True:
            try:
                changed_file = self.queue.get(block=block, timeout=timeout)
                changed_files.append(changed_file)
            except Empty:
                break
        return changed_files


# DEBOUNCE_SECONDS = 4
# LAST_TIME = 0.0
# WATCHED_FILES: list[str] = []

# def debounced_sketch_filewatcher_get_all_changes() -> list[str]:
#     nonlocal DEBOUNCE_SECONDS
#     nonlocal LAST_TIME
#     nonlocal WATCHED_FILES
#     current_time = time.time()
#     new_files = sketch_filewatcher.get_all_changes()
#     if new_files:
#         WATCHED_FILES.extend(new_files)
#         print(f"Changes detected in {new_files}")
#         LAST_TIME = current_time
#         return []
#     diff = current_time - LAST_TIME
#     if diff > DEBOUNCE_SECONDS and len(WATCHED_FILES) > 0:
#         LAST_TIME = current_time
#         WATCHED_FILES, changed_files = [], WATCHED_FILES
#         changed_files = sorted(list(set(changed_files)))
#         return changed_files
#     return []


class DebouncedFileWatcherProcess:
    """
    Wraps a FileWatcherProcess to batch rapid-fire change events
    and only emit them once the debounce interval has passed.
    """

    def __init__(
        self,
        watcher: FileWatcherProcess,
        debounce_seconds: float = FILE_CHANGED_DEBOUNCE_SECONDS,
    ) -> None:
        self.watcher = watcher
        self.debounce_seconds = debounce_seconds
        self._last_time = 0.0
        self._watched_files: list[str] = []

    def get_all_changes(self, timeout: float | None = None) -> list[str]:
        """
        Polls the underlying watcher for raw events, accumulates them,
        and once no new events arrive for `debounce_seconds`, flushes
        a sorted, unique list of paths.
        """
        now = time.time()
        # pull in any new raw events
        new = self.watcher.get_all_changes(timeout=timeout)
        if new:
            self._watched_files.extend(new)
            # reset the window
            self._last_time = now
            return []

        # if the window has elapsed, flush
        if self._watched_files and (now - self._last_time) > self.debounce_seconds:
            batch = sorted(set(self._watched_files))
            self._watched_files.clear()
            self._last_time = now
            return batch

        return []

    def stop(self) -> None:
        """Tear down the underlying watcher process."""
        self.watcher.stop()
