

import os
from shutil import which, copytree, rmtree
from pathlib import Path
import subprocess

CSS_CONTENT = """body {
    background-color: #121212;
    color: #E0E0E0;
    margin: 0;
    padding: 20px;
    font-family: 'Roboto Condensed', sans-serif;
    display: flex;
    flex-direction: column;
    align-items: center;
    min-height: 100vh;
    max-width: 1000px;
    margin: 0 auto;
}

h1 {
    font-size: 6em;
    margin-top: 10vh;
    margin-bottom: 40px;
    text-align: center;
    font-weight: 300;
    letter-spacing: 1px;
    line-height: 1.2;
    position: relative;
    animation: continuousGlow 4s ease-in-out infinite;
}

@keyframes continuousGlow {
    0% {
        text-shadow: 0 0 5px rgba(224, 224, 224, 0.1);
    }
    25% {
        text-shadow: 0 0 20px rgba(224, 224, 224, 0.3);
    }
    50% {
        text-shadow: 0 0 30px rgba(224, 224, 224, 0.5);
    }
    75% {
        text-shadow: 0 0 20px rgba(224, 224, 224, 0.3);
    }
    100% {
        text-shadow: 0 0 5px rgba(224, 224, 224, 0.1);
    }
}

.example-link {
    display: block;
    padding: 10px;
    margin: 5px 0;
    text-decoration: none;
    color: #E0E0E0;
    background-color: #1E1E1E;
    border-radius: 5px;
    transition: background-color 0.2s ease-in-out, box-shadow 0.2s ease-in-out;
}

.example-link:hover {
    background-color: #2E2E2E;
    box-shadow: 0 0 10px rgba(255, 255, 255, 0.1);
}
"""

INDEX_TEMPLATE = """<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>FastLED Examples</title>
    <link href="https://fonts.googleapis.com/css2?family=Roboto+Condensed:wght@300&display=swap" rel="stylesheet">
    <link rel="stylesheet" href="index.css">
</head>
<body>
    <h1>FastLED Examples</h1>
    {example_links}
</body>
</html>
"""

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


def generate_css(outputdir: Path | None = None) -> None:
    outputdir = outputdir or DOCS
    css_file = outputdir / "index.css"
    with open(css_file, "w") as f:
        f.write(CSS_CONTENT)

def build_index_html(outputdir: Path | None = None) -> None:
    outputdir = outputdir or DOCS
    assert outputdir.exists(), f"Output directory {outputdir} not found, you should run build_example first"
    index_html = outputdir / "index.html"
    
    examples = [f for f in outputdir.iterdir() if f.is_dir()]
    examples = sorted(examples)
    
    example_links = '\n'.join(
        f'    <a class="example-link" href="{example.name}/index.html">{example.name}</a>'
        for example in examples
    )
    
    with open(index_html, "w") as f:
        f.write(INDEX_TEMPLATE.format(example_links=example_links))



def main() -> int:
    for example in EXAMPLES:
        build_example(example)
    generate_css()
    build_index_html()
    return 0


if __name__ == "__main__":
    main()
