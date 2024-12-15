#!/usr/bin/env python3
import subprocess
import sys
import os

from pathlib import Path

HERE = Path(__file__).parent

def check_pyinstaller():
    """Check if pyinstaller is installed, install if not."""
    try:
        subprocess.run([sys.executable, "-m", "pip", "show", "pyinstaller"], 
                      check=True, capture_output=True)
    except subprocess.CalledProcessError:
        print("Installing pyinstaller...")
        subprocess.run([sys.executable, "-m", "pip", "install", "pyinstaller"], 
                      check=True)

def main():
    os.chdir(str(HERE))
    # Activate virtual environment if needed
    # Note: This is typically handled by the Python interpreter itself
    # when running the script from the correct environment
    
    # Check and install pyinstaller if needed
    check_pyinstaller()
    
    # Run pyinstaller with the same arguments as the bash script
    cmd = [
        "pyinstaller",
        "src/fastled/cli.py",
        "--onefile",
        "--clean",
        "--argv-emulation",
        "--name", "fastled",
        #"--target-architecture", "universal2"
    ]
    
    try:
        subprocess.run(cmd, check=True)
    except subprocess.CalledProcessError as e:
        print(f"Error running pyinstaller: {e}")
        return 1
    return 0

if __name__ == "__main__":
    sys.exit(main())
