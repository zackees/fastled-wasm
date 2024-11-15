"""
Main entry point.
"""

import sys

from fastled.app import main as app_main


def main() -> int:
    """Main entry point for the template_python_cmd package."""
    return app_main()


if __name__ == "__main__":
    sys.exit(main())
