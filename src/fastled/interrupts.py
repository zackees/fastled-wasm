"""Helpers for propagating KeyboardInterrupt across threads."""

from __future__ import annotations

import _thread
import threading


def notify_main_thread() -> None:
    """Wake the main thread when a worker thread receives Ctrl+C."""
    if threading.current_thread() is not threading.main_thread():
        _thread.interrupt_main()


def handle_keyboard_interrupt(ki: KeyboardInterrupt) -> None:
    """Propagate an interrupt without swallowing it."""
    notify_main_thread()
    if threading.current_thread() is threading.main_thread():
        raise KeyboardInterrupt() from ki
