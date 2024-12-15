

import os

import shutil

if not shutil.which("fastled"):
    raise FileNotFoundError("fastled executable not found")


def test_examples():
    os.system("fastled test-examples")
    assert os.path.exists("examples")


