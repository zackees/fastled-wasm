import zipfile
from pathlib import Path

import httpx

from fastled.env import DEFAULT_URL

ENDPOINT = f"{DEFAULT_URL}/project/init"


def project_init() -> Path:
    """
    Initialize a new FastLED project.
    """
    response = httpx.get(ENDPOINT, timeout=20)
    response.raise_for_status()
    content = response.content
    output = Path("fastled.zip")
    output.write_bytes(content)
    # unzip the content
    outdir = Path("fastled")
    if outdir.exists():
        print("Project already initialized.")
        return Path("fastled").iterdir().__next__()
    with zipfile.ZipFile(output, "r") as zip_ref:
        zip_ref.extractall(outdir)
    print(f"Project initialized successfully at {outdir}")
    output.unlink()
    return Path("fastled").iterdir().__next__()


def unit_test() -> None:
    project_init()


if __name__ == "__main__":
    unit_test()
