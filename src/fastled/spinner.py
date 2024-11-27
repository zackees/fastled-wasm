import _thread
import threading
import time
import warnings

from progress.spinner import Spinner as SpinnerImpl


class Spinner:
    def __init__(self, message: str = ""):
        self.spinner = SpinnerImpl(message)
        self.event = threading.Event()
        self.thread = threading.Thread(target=self._spin, daemon=True)
        self.thread.start()

    def _spin(self) -> None:
        try:
            while not self.event.is_set():
                self.spinner.next()
                time.sleep(0.1)
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
