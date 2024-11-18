import os
import select
import sys
import time
from multiprocessing import Process, Queue
from queue import Empty


class SpaceBarWatcher:
    def __init__(self) -> None:
        self.queue: Queue = Queue()
        self.queue_cancel: Queue = Queue()
        self.process = Process(target=self._watch_for_space)
        self.process.start()

    def _watch_for_space(self) -> None:
        # Set stdin to non-blocking mode
        fd = sys.stdin.fileno()
        if os.name != "nt":  # Unix-like systems
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
                        if char == " ":
                            self.queue.put(ord(" "))
            finally:
                termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)  # type: ignore
        else:  # Windows
            import msvcrt

            while True:
                # Check for cancel signal
                try:
                    self.queue_cancel.get(timeout=0.1)
                    break
                except Empty:
                    pass

                # Check if there's input ready
                if msvcrt.kbhit():
                    char = msvcrt.getch().decode()
                    if char == " ":
                        self.queue.put(ord(" "))

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
