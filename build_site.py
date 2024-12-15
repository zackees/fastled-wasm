

import os
from shutil import which, copytree, rmtree
from pathlib import Path
import subprocess

HERE = Path(__file__).parent.resolve()
DOCS = HERE / "site"

EXAMPLES = [
    "wasm",
    "Chromancer",
    "SdCard",
]

def _exec(cmd: str) -> None:
    subprocess.run(cmd, shell=True, check=True)

def build_example(example: str, outputdir: Path | None = None) -> None:
    if not which("fastled"):
        raise FileNotFoundError("fastled executable not found")
    outputdir = outputdir or DOCS
    src_dir = outputdir / example / "src"
    _exec(f"fastled --init={example} {src_dir}")
    assert src_dir.exists()
    _exec(f"fastled {src_dir / example} --just-compile")
    fastled_dir = src_dir / example / "fastled_js"
    assert fastled_dir.exists(), f"fastled dir {fastled_dir} not found"
    # now copy it to the example dir
    example_dir = outputdir / example
    copytree(fastled_dir, example_dir, dirs_exist_ok=True)
    # now remove the src dir
    rmtree(src_dir, ignore_errors=True)
    print(f"Built {example} example in {example_dir}")
    assert (example_dir / "fastled.wasm").exists()


def gen_index_html(outputdir: Path | None = None) -> None:
    outputdir = outputdir or DOCS
    assert outputdir.exists(), f"Output directory {outputdir} not found, you should run build_example first"
    index_html = outputdir / "index.html"
    # scan the example folders and generate an index.html. Each example
    # will be a link to the example's index.html
    examples = [f for f in outputdir.iterdir() if f.is_dir()]
    examples = sorted(examples)
    with open(index_html, "w") as f:
        f.write("<html><body>\n")
        f.write("<h1>FastLED Examples</h1>\n")
        for example in examples:
            f.write(f'<a href="{example.name}/index.html">{example.name}</a><br>\n')
        f.write("</body></html>\n")



def main() -> int:
    for example in EXAMPLES:
        build_example(example)
    return 0


if __name__ == "__main__":
    main()