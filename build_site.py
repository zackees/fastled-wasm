

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
    position: relative;
    width: 100%;
    height: 100vh;
    overflow-x: hidden;
}

.nav-trigger {
    position: fixed;
    left: 10px;
    top: 10px;
    width: 40px;
    height: 40px;
    z-index: 1001;
    background-color: #252525;
    border-radius: 5px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
}

/* Hamburger icon */
.nav-trigger i {
    color: #E0E0E0;
    font-size: 24px;
}

.nav-pane {
    position: fixed;
    left: 10px;
    top: 60px;
    width: 250px;
    height: auto;
    background-color: rgba(30, 30, 30, 0.95);
    border-radius: 5px;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.5);
    transform: translateY(-20px);
    opacity: 0;
    pointer-events: none;
    transition: transform 0.3s ease, opacity 0.3s ease;
}

.nav-pane.visible {
    transform: translateY(0);
    opacity: 1;
    pointer-events: auto;
}

.main-content {
    width: 100%;
    height: 100%;
    padding: 0;
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
    margin: 5px 10px;
    padding: 15px 10px;
    border-radius: 5px;
    display: block;
    text-decoration: none;
    color: #E0E0E0;
    background-color: #252525;
    transition: background-color 0.2s ease-in-out, box-shadow 0.2s ease-in-out;
    position: relative;
    padding-right: 35px;  /* Make room for checkmark */
}


.example-link:hover {
    background-color: #2E2E2E;
    box-shadow: 0 0 10px rgba(255, 255, 255, 0.1);
}

.example-link:last-child {
    margin-bottom: 10px;
}
"""

INDEX_TEMPLATE = """<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>FastLED Examples</title>
    <link href="https://fonts.googleapis.com/css2?family=Roboto+Condensed:wght@300&display=swap" rel="stylesheet">
    <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.4.0/css/all.min.css">
    <link rel="stylesheet" href="index.css">
</head>
<body>
    <div class="content-wrapper">
        <div class="nav-trigger"><i class="fas fa-bars"></i></div>
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
            const navPane = document.querySelector('.nav-pane');
            const navTrigger = document.querySelector('.nav-trigger');
            
            // First add checkmarks to all links
            links.forEach(link => {{
                // Add the checkmark span to each link
                const checkmark = document.createElement('i');
                checkmark.className = 'fas fa-check';
                checkmark.style.display = 'none';
                checkmark.style.position = 'absolute';
                checkmark.style.right = '10px';
                checkmark.style.top = '50%';
                checkmark.style.transform = 'translateY(-50%)';
                checkmark.style.color = '#4CAF50';
                link.appendChild(checkmark);
            }});
            
            // Now load first example and show its checkmark
            if (links.length > 0) {{
                iframe.src = links[0].getAttribute('href');
                links[0].classList.add('active');
                links[0].querySelector('.fa-check').style.display = 'inline-block';
            }}
            
            // Add click handlers
            links.forEach(link => {{
                link.addEventListener('click', function(e) {{
                    e.preventDefault();
                    // Hide all checkmarks
                    links.forEach(l => {{
                        l.querySelector('.fa-check').style.display = 'none';
                        l.classList.remove('active');
                    }});
                    // Show this checkmark
                    this.querySelector('.fa-check').style.display = 'inline-block';
                    this.classList.add('active');
                    iframe.src = this.getAttribute('href');
                    hideNav();  // Hide nav after selection
                }});
            }});

            function showNav() {{
                navPane.classList.add('visible');
                navPane.style.opacity = '1';
            }}

            function hideNav() {{
                navPane.style.opacity = '0';  // Start fade out
                setTimeout(() => {{
                    navPane.classList.remove('visible');
                }}, 300);
            }}

            // Click handlers for nav
            navTrigger.addEventListener('click', (e) => {{
                e.stopPropagation();
                if (navPane.classList.contains('visible')) {{
                    hideNav();
                }} else {{
                    showNav();
                }}
            }});
            
            // Close menu when clicking anywhere in the document
            document.addEventListener('click', (e) => {{
                if (navPane.classList.contains('visible') && 
                    !navPane.contains(e.target) && 
                    !navTrigger.contains(e.target)) {{
                    hideNav();
                }}
            }});

            // Close when clicking iframe
            iframe.addEventListener('load', () => {{
                iframe.contentDocument?.addEventListener('click', () => {{
                    if (navPane.classList.contains('visible')) {{
                        hideNav();
                    }}
                }});
            }});

            // Initial state
            hideNav();
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
    #with open(css_file, "w") as f:
    #    f.write(CSS_CONTENT, encoding="utf-8")
    css_file.write_text(CSS_CONTENT, encoding="utf-8")

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
