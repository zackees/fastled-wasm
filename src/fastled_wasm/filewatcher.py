"""File system watcher implementation using watchdog
"""

import queue
import threading
import time
from typing import Dict, Optional

from watchdog.events import FileSystemEvent, FileSystemEventHandler
from watchdog.observers import Observer
from watchdog.observers.api import BaseObserver


class MyEventHandler(FileSystemEventHandler):
    def __init__(self, change_queue: queue.Queue) -> None:
        super().__init__()
        self.change_queue = change_queue

    def on_modified(self, event: FileSystemEvent) -> None:
        if not event.is_directory:
            self.change_queue.put(event.src_path)


class FileChangedNotifier(threading.Thread):
    """Watches a directory for file changes and queues notifications"""

    def __init__(self, path: str, debounce_seconds: float = 0.1) -> None:
        """Initialize the notifier with a path to watch

        Args:
            path: Directory path to watch for changes
            debounce_seconds: Minimum time between notifications for the same file
        """
        super().__init__()
        self.path = path
        self.observer: BaseObserver | None = None
        self.event_handler: MyEventHandler | None = None
        self.stopped = threading.Event()
        self.change_queue: queue.Queue = queue.Queue()
        self.last_notification: Dict[str, float] = {}
        self.debounce_seconds = debounce_seconds

    def stop(self) -> None:
        """Stop watching for changes"""
        self.stopped.set()
        if self.observer:
            self.observer.stop()
            self.observer.join()
            self.observer = None
            self.event_handler = None

    def run(self) -> None:
        """Thread main loop - starts watching for changes"""
        self.event_handler = MyEventHandler(self.change_queue)
        self.observer = Observer()
        self.observer.schedule(self.event_handler, self.path, recursive=True)
        self.observer.start()

        try:
            while not self.stopped.is_set():
                time.sleep(0.1)
        finally:
            self.stop()

    def get_next_change(self, timeout: float = 0.1) -> Optional[str]:
        """Get the next file change event from the queue

        Args:
            timeout: How long to wait for next change in seconds

        Returns:
            Changed filepath or None if no change within timeout
        """
        try:
            filepath = self.change_queue.get(timeout=timeout)
            current_time = time.time()

            # Check if we've seen this file recently
            last_time = self.last_notification.get(filepath, 0)
            if current_time - last_time < self.debounce_seconds:
                return None

            self.last_notification[filepath] = current_time
            return filepath
        except queue.Empty:
            return None
