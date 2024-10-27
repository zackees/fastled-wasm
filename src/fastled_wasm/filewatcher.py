"""File system watcher implementation using watchdog
"""

import time
from typing import Callable

from watchdog.events import FileSystemEvent, FileSystemEventHandler
from watchdog.observers import Observer
from watchdog.observers.api import BaseObserver


class MyEventHandler(FileSystemEventHandler):
    def __init__(self, callback: Callable[[str], None]) -> None:
        super().__init__()
        self.callback = callback

    def on_modified(self, event: FileSystemEvent) -> None:
        if not event.is_directory:
            self.callback(event.src_path)


class FileChangedNotifier:
    """Watches a directory for file changes and notifies via callback"""

    def __init__(self, path: str) -> None:
        """Initialize the notifier with a path to watch

        Args:
            path: Directory path to watch for changes
        """
        self.path = path
        self.observer: BaseObserver | None = None
        self.event_handler: MyEventHandler | None = None
        self.stopped: bool = True

    def stop(self) -> None:
        """Stop watching for changes"""
        if self.observer:
            self.observer.stop()
            self.observer.join()
            self.observer = None
            self.event_handler = None
            self.stopped = True

    def start(self, on_change: Callable[[str], None]) -> None:
        """Start watching for changes

        Args:
            on_change: Callback function that takes changed filepath as argument
        """
        self.stop()  # Stop any existing watch
        self.stopped = False
        self.event_handler = MyEventHandler(on_change)
        self.observer = Observer()
        self.observer.schedule(self.event_handler, self.path, recursive=True)
        self.observer.start()

        try:
            while True and not self.stopped:
                time.sleep(0.1)
        except KeyboardInterrupt:
            self.stop()
        finally:
            self.stop()
