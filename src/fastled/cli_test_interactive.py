import sys

from fastled.app import main as app_main

if __name__ == "__main__":
    # Note that the entry point for the exe is in cli.py
    try:
        import os

        os.chdir("../fastled")
        # sys.argv.append("--server")
        # sys.argv.append("--local")
        sys.argv.append("examples/FxWave2d")
        sys.argv.append("-i")
        sys.exit(app_main())
    except KeyboardInterrupt:
        print("\nExiting from main...")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
