import zipfile
from pathlib import Path

import httpx

from fastled.env import DEFAULT_URL

ENDPOINT_PROJECT_INIT = f"{DEFAULT_URL}/project/init"
ENDPOINT_INFO = f"{DEFAULT_URL}/info"
DEFAULT_EXAMPLE = "wasm"


def get_examples() -> list[str]:
    response = httpx.get(ENDPOINT_INFO, timeout=4)
    response.raise_for_status()
    return response.json()["examples"]


def project_init(example: str | None = None, outputdir: Path | None = None) -> Path:
    """
    Initialize a new FastLED project.
    """

    outputdir = outputdir or Path("fastled")
    if example is None:
        try:
            examples = get_examples()
            print("Available examples:")
            for i, example in enumerate(examples):
                print(f"  {i+1}: {example}")
            example_num = int(input("Enter the example number: ")) - 1
            example = examples[example_num]
        except httpx.HTTPStatusError:
            print(
                f"Failed to fetch examples, using default example '{DEFAULT_EXAMPLE}'"
            )
            example = DEFAULT_EXAMPLE
    assert example is not None
    response = httpx.get(f"{ENDPOINT_PROJECT_INIT}/{example}", timeout=20)
    response.raise_for_status()
    content = response.content
    tmpzip = outputdir / "fastled.zip"
    tmpzip.write_bytes(content)
    with zipfile.ZipFile(tmpzip, "r") as zip_ref:
        zip_ref.extractall(outputdir)
    print(f"Project initialized successfully at {outputdir.resolve()}")
    tmpzip.unlink()
    return outputdir.iterdir().__next__()


def unit_test() -> None:
    project_init()


if __name__ == "__main__":
    unit_test()
