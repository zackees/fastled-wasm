import zipfile
from pathlib import Path

import httpx

from fastled.settings import DEFAULT_URL

DEFAULT_EXAMPLE = "wasm"


def get_examples(host: str | None = None) -> list[str]:
    host = host or DEFAULT_URL
    url_info = f"{host}/info"
    response = httpx.get(url_info, timeout=4)
    response.raise_for_status()
    examples: list[str] = response.json()["examples"]
    return sorted(examples)


def _prompt_for_example() -> str:
    examples = get_examples()
    while True:
        print("Available examples:")
        for i, example in enumerate(examples):
            print(f"  [{i+1}]: {example}")
        answer = input("Enter the example number or name: ").strip()
        if answer.isdigit():
            example_num = int(answer) - 1
            if example_num < 0 or example_num >= len(examples):
                print("Invalid example number")
                continue
            return examples[example_num]
        elif answer in examples:
            return answer


def project_init(
    example: str | None = "PROMPT",  # prompt for example
    outputdir: Path | None = None,
    host: str | None = None,
) -> Path:
    """
    Initialize a new FastLED project.
    """
    host = host or DEFAULT_URL
    outputdir = Path(outputdir) if outputdir is not None else Path("fastled")
    outputdir.mkdir(exist_ok=True, parents=True)
    if example == "PROMPT" or example is None:
        try:
            example = _prompt_for_example()
        except httpx.HTTPStatusError:
            print(
                f"Failed to fetch examples, using default example '{DEFAULT_EXAMPLE}'"
            )
            example = DEFAULT_EXAMPLE
    assert example is not None
    endpoint_url = f"{host}/project/init"
    json = example
    response = httpx.post(endpoint_url, timeout=20, json=json)
    response.raise_for_status()
    content = response.content
    tmpzip = outputdir / "fastled.zip"
    outputdir.mkdir(exist_ok=True)
    tmpzip.write_bytes(content)
    with zipfile.ZipFile(tmpzip, "r") as zip_ref:
        zip_ref.extractall(outputdir)
    tmpzip.unlink()
    out = outputdir / example
    print(f"Project initialized at {out}")
    assert out.exists()
    return out


def unit_test() -> None:
    project_init()


if __name__ == "__main__":
    unit_test()
