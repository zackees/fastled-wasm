#!/usr/bin/env python3
"""
FastLED Package Main Entry Point
Enables running the package as: python -m fastled
"""

import sys

from fastled.cli import main

if __name__ == "__main__":
    # Pass execution to the main function from cli.py
    # This enables 'python -m fastled' to work the same as 'fastled' command
    sys.exit(main())
