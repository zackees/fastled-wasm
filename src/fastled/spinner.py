import _thread
import itertools
import sys
import threading
import time
import warnings


class Spinner:
    _FRAMES = "|/-\\"

    def __init__(self, message: str = ""):
        self.message = message
        self.event = threading.Event()
        self.thread = threading.Thread(target=self._spin, daemon=True)
        self.thread.start()

    def _spin(self) -> None:
        try:
            for frame in itertools.cycle(self._FRAMES):
                if self.event.is_set():
                    break
                sys.stderr.write(f"\r{self.message} {frame}")
                sys.stderr.flush()
                time.sleep(0.1)
            sys.stderr.write("\r" + " " * (len(self.message) + 2) + "\r")
            sys.stderr.flush()
        except KeyboardInterrupt:
            _thread.interrupt_main()
        except Exception as e:
            warnings.warn(f"Spinner thread failed: {e}")

    def stop(self) -> None:
        self.event.set()
        self.thread.join()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.stop()
