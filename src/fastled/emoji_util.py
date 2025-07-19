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
