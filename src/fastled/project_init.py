import zipfile
from pathlib import Path

import httpx

from fastled.env import DEFAULT_URL

ENDPOINT_PROJECT_INIT = f"{DEFAULT_URL}/project/init"
ENDPOINT_INFO = f"{DEFAULT_URL}/info"


def _get_examples() -> list[str]:
    response = httpx.get(ENDPOINT_INFO, timeout=4)
    response.raise_for_status()
    return response.json()["examples"]


def project_init() -> Path:
    """
    Initialize a new FastLED project.
    """

    example = "wasm"
    try:
        examples = _get_examples()
        print("Available examples:")
        for i, example in enumerate(examples):
            print(f"  {i+1}: {example}")
        example_num = int(input("Enter the example number: ")) - 1
        example = examples[example_num]
    except httpx.HTTPStatusError:
        print(f"Failed to fetch examples, using default example '{example}'")
    response = httpx.get(f"{ENDPOINT_PROJECT_INIT}/{example}", timeout=20)
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
