import sys

from fastled.app import main as app_main
from fastled.interrupts import handle_keyboard_interrupt

if __name__ == "__main__":
    # Note that the entry point for the exe is in cli.py
    try:
        import os

        os.chdir("../fastled")
        sys.argv.append("examples/Corkscrew")
        sys.exit(app_main())
    except KeyboardInterrupt as ki:
        print("\nExiting from main...")
        handle_keyboard_interrupt(ki)
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
