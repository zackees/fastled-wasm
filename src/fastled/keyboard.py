import os
import select
import sys
import time
from threading import Thread
from queue import Queue, Empty

_WHITE_SPACE = [" ", "\r", "\n"]


# Original space bar, but now also enter key.
class SpaceBarWatcher:

    @classmethod
    def watch_space_bar_pressed(cls, timeout: float = 0) -> bool:
        watcher = cls()
        try:
            start_time = time.time()
            while True:
                if watcher.space_bar_pressed():
                    return True
                if time.time() - start_time > timeout:
                    return False
        finally:
            watcher.stop()

    def __init__(self) -> None:
        self.queue: Queue = Queue()
        self.queue_cancel: Queue = Queue()
        self.process = Thread(target=self._watch_for_space, daemon=True)
        self.process.start()

    def _watch_for_space(self) -> None:
        # Set stdin to non-blocking mode
        fd = sys.stdin.fileno()

        if os.name == "nt":  # Windows
            import msvcrt

            while True:
                # Check for cancel signal
                try:
                    self.queue_cancel.get(timeout=0.1)
                    break
                except Empty:
                    pass

                # Check if there's input ready
                if msvcrt.kbhit():  # type: ignore
                    char = msvcrt.getch().decode()  # type: ignore
                    if char in _WHITE_SPACE:
                        self.queue.put(ord(" "))

        else:  # Unix-like systems
            import termios
            import tty

            old_settings = termios.tcgetattr(fd)  # type: ignore
            try:
                tty.setraw(fd)  # type: ignore
                while True:
                    # Check for cancel signal
                    try:
                        self.queue_cancel.get(timeout=0.1)
                        break
                    except Empty:
                        pass

                    # Check if there's input ready
                    if select.select([sys.stdin], [], [], 0.1)[0]:
                        char = sys.stdin.read(1)
                        if char in _WHITE_SPACE:
                            self.queue.put(ord(" "))
            finally:
                termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)  # type: ignore

    def space_bar_pressed(self) -> bool:
        found = False
        while not self.queue.empty():
            try:
                key = self.queue.get(block=False, timeout=0.1)
                if key == ord(" "):
                    found = True
            except Empty:
                break
        return found

    def stop(self) -> None:
        self.queue_cancel.put(True)
        self.process.join()
        self.queue.join()


def main() -> None:
    watcher = SpaceBarWatcher()
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
