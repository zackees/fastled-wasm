import curses
import time
from multiprocessing import Process, Queue
from queue import Empty
from typing import Any


class SpaceBoardWatcherProcess:
    def __init__(self) -> None:
        self.queue: Queue = Queue()
        self.queue_cancel: Queue = Queue()
        self.process = Process(target=self._watch_for_space)
        self.process.start()

    def _watch_for_space(self) -> None:
        def _curses_main(stdscr: Any) -> None:
            """Main function for the curses application."""
            stdscr.nodelay(True)  # Non-blocking input
            stdscr.addstr("Press the space bar to exit...\n")
            while True:
                try:
                    self.queue_cancel.get(timeout=0.5)
                except Empty:
                    pass
                key: int = stdscr.getch()
                if key == ord(" "):  # ASCII code for space
                    stdscr.addstr("Space bar pressed!\n")
                    self.queue.put(key)

        curses.wrapper(_curses_main)

    def space_bar_pressed(self) -> bool:
        found = False
        while not self.queue.empty():
            key = self.queue.get()
            if key == ord(" "):
                found = True
        return found

    def stop(self) -> None:
        self.queue_cancel.put(True)
        self.process.terminate()
        self.process.join()
        self.queue.close()
        self.queue.join_thread()


def main() -> None:
    watcher = SpaceBoardWatcherProcess()
    try:
        while True:
            if watcher.space_bar_pressed():
                break
            time.sleep(1)
    finally:
        watcher.stop()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("Keyboard interrupt detected.")
        pass
