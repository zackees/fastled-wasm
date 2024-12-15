"""
Main entry point.
"""

import multiprocessing
import sys

from fastled.app import main as app_main


def main() -> int:
    """Main entry point for the template_python_cmd package."""
    return app_main()


# Cli entry point for the pyinstaller generated exe
if __name__ == "__main__":
    multiprocessing.freeze_support()  # needed by pyinstaller.
    sys.exit(main())
