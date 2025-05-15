"""
Main entry point.
"""

import multiprocessing
import sys


def run_app() -> int:
    """Run the application."""
    from fastled.app import main as app_main

    return app_main()


def main() -> int:
    """Main entry point for the template_python_cmd package."""
    # if "--debug" in sys.argv:
    #     # Debug mode
    #     os.environ["FLASK_SERVER_LOGGING"] = "1"
    return run_app()


# Cli entry point for the pyinstaller generated exe
if __name__ == "__main__":
    multiprocessing.freeze_support()  # needed by pyinstaller.
    sys.exit(main())
