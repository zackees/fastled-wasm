"""
Emoji utility functions for handling Unicode display issues on Windows cmd.exe
"""

import sys


def EMO(emoji: str, fallback: str) -> str:
    """Get emoji with fallback for systems that don't support Unicode properly"""
    try:
        # Test if we can encode the emoji properly
        emoji.encode(sys.stdout.encoding or "utf-8")
        return emoji
    except (UnicodeEncodeError, AttributeError):
        return fallback


def safe_print(text: str) -> None:
    """Print text safely, handling Unicode/emoji encoding issues on Windows cmd.exe"""
    try:
        print(text)
    except UnicodeEncodeError:
        # Replace problematic characters with safe alternatives
        safe_text = text.encode(
            sys.stdout.encoding or "utf-8", errors="replace"
        ).decode(sys.stdout.encoding or "utf-8")
        print(safe_text)
