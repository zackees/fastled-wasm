

import os
import argparse
from shutil import which, copytree, rmtree
from pathlib import Path
import subprocess

CSS_CONTENT = """
body {
    background-color: #121212;
    color: #E0E0E0;
    margin: 0;
    padding: 0;
    font-family: 'Roboto Condensed', sans-serif;
    min-height: 100vh;
    display: grid;
    grid-template-rows: 1fr;
}

.content-wrapper {
    position: relative;  /* Changed from grid to relative positioning */
    width: 100%;
    height: 100vh;
}

.nav-trigger {
    position: fixed;
    left: 0;
    top: 0;
    width: 250px;
    height: 100%;
    z-index: 999;
    background-color: transparent; /* Add this to ensure it's clickable */
}

.nav-pane {
    position: fixed;
    left: 0;
    top: 0;
    width: 250px;
    height: 100%;
    background-color: #1E1E1E;
    padding: 20px;
    border-right: 1px solid #333;
    opacity: 1;
    transition: opacity .5s ease;
    z-index: 1000;
    box-sizing: border-box;
    pointer-events: auto;
    display: block;
}

.main-content {
    width: 100%;
    height: 100%;
    padding: 0;         /* Remove padding */
    overflow: hidden;
}

#example-frame {
    width: 100%;
    height: 100%;
    border: none;
    background-color: #121212;
    overflow: auto;
}

.example-link {
    display: block;
    padding: 10px;
    margin: 5px 0;
    text-decoration: none;
    color: #E0E0E0;
    background-color: #252525;
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
    <div class="content-wrapper">
        <div class="nav-trigger"></div>
        <nav class="nav-pane">
            {example_links}
        </nav>
        <main class="main-content">
            <iframe id="example-frame" title="Example Content"></iframe>
        </main>
    </div>
    <script>
        document.addEventListener('DOMContentLoaded', function() {{
            const links = document.querySelectorAll('.example-link');
            const iframe = document.getElementById('example-frame');
            
            // Load first example by default
            if (links.length > 0) {{
                iframe.src = links[0].getAttribute('href');
            }}
            
            links.forEach(link => {{
                link.addEventListener('click', function(e) {{
                    e.preventDefault();
                    iframe.src = this.getAttribute('href');
                }});
            }});

            // Navigation pane visibility handling
            const navPane = document.querySelector('.nav-pane');
            const navTrigger = document.querySelector('.nav-trigger');

            function showNav() {{
                navPane.style.opacity = '1';
                navPane.style.pointerEvents = 'auto';
            }}

            function hideNav() {{
                navPane.style.opacity = '0';
                navPane.style.pointerEvents = 'none';
            }}

            navTrigger.addEventListener('mouseenter', showNav);
            navPane.addEventListener('mouseenter', showNav);
            navTrigger.addEventListener('mouseleave', hideNav);
            navPane.addEventListener('mouseleave', hideNav);

            // Add initial timeout to hide nav after 1 second
            setTimeout(hideNav, 1000);
        }});
    </script>
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



def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description='Build FastLED example site')
    parser.add_argument('--fast', action='store_true', 
                       help='Skip regenerating existing examples, only rebuild index.html and CSS')
    return parser.parse_args()

def main() -> int:
    args = parse_args()
    
    for example in EXAMPLES:
        example_dir = DOCS / example
        if not args.fast or not example_dir.exists():
            build_example(example)
    
    generate_css()
    build_index_html()
    return 0


if __name__ == "__main__":
    main()
