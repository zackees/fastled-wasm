import sys
import time
import os
import select
from threading import Thread
from queue import Queue, Empty
from typing import Any


class SpaceBarWatcher:
    def __init__(self) -> None:
        self.queue: Queue = Queue()
        self.queue_cancel: Queue = Queue()
        self.thread = Thread(target=self._watch_for_space, daemon=True)
        self.thread.start()

    def _watch_for_space(self) -> None:
        print("Press space bar to stop the process.")
        # Set stdin to non-blocking mode
        fd = sys.stdin.fileno()
        if os.name != 'nt':  # Unix-like systems
            import tty
            import termios
            old_settings = termios.tcgetattr(fd)
            try:
                tty.setraw(fd)
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
                        if char == ' ':
                            self.queue.put(ord(' '))
            finally:
                termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)
        else:  # Windows
            import msvcrt
            while True:
                # Check for cancel signal
                try:
                    self.queue_cancel.get(timeout=0.1)
                    break
                except Empty:
                    pass

                print("Checking for key press")

                # Check if there's input ready
                if msvcrt.kbhit():
                    char = msvcrt.getch().decode()
                    print(f"Got key press: {char}")
                    if char == ' ':
                        self.queue.put(ord(' '))

    def space_bar_pressed(self) -> bool:
        found = False
        while not self.queue.empty():
            key = self.queue.get()
            if key == ord(" "):
                found = True
        return found

    def stop(self) -> None:
        self.queue_cancel.put(True)
        self.thread.join()


def main() -> None:
    watcher = SpaceBarWatcher()
    try:
        while True:
            if watcher.space_bar_pressed():
                break
            time.sleep(.1)
    finally:
        watcher.stop()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("Keyboard interrupt detected.")
