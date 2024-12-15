

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
    left: 0;
    top: 0;
    width: 250px;
    height: 100%;
    z-index: 999;
    background-color: transparent;
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

@media (max-width: 768px) {
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
    .nav-trigger::before {
        content: '===';
        color: #E0E0E0;
        font-size: 20px;
        font-weight: bold;
        letter-spacing: -2px;  /* Bring the equals signs closer together */
    }

    .nav-pane {
        position: fixed;
        left: 10px;
        top: 60px;  /* Position below trigger button */
        width: 250px;  /* Fixed width dropdown */
        height: auto;
        background-color: rgba(30, 30, 30, 0.95);
        border-radius: 5px;
        box-shadow: 0 2px 10px rgba(0, 0, 0, 0.5);
        transform: translateY(-20px);
        opacity: 0;
        pointer-events: none;
        transition: transform 0.3s ease, opacity 0.3s ease;  /* Slightly longer transition */
    }

    .nav-pane.visible {
        transform: translateY(0);
        opacity: 1;
        pointer-events: auto;
    }

    .example-link {
        margin: 5px 10px;
        padding: 15px 10px;
        border-radius: 5px;
    }

    .example-link:last-child {
        margin-bottom: 10px;
    }
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
            const navPane = document.querySelector('.nav-pane');
            const navTrigger = document.querySelector('.nav-trigger');
            
            // Load first example by default
            if (links.length > 0) {{
                iframe.src = links[0].getAttribute('href');
            }}
            
            links.forEach(link => {{
                link.addEventListener('click', function(e) {{
                    e.preventDefault();
                    iframe.src = this.getAttribute('href');
                    if (isMobile()) {{
                        hideNav();  // Hide nav after selection on mobile
                    }}
                }});
            }});

            function isMobile() {{
                return window.innerWidth <= 768;
            }}

            function showNav() {{
                if (isMobile()) {{
                    navPane.classList.add('visible');
                }} else {{
                    navPane.style.opacity = '1';
                    navPane.style.pointerEvents = 'auto';
                }}
            }}

            function hideNav() {{
                if (isMobile()) {{
                    navPane.style.opacity = '0';  // Start fade out
                    // Wait for fade before removing visible class
                    setTimeout(() => {{
                        navPane.classList.remove('visible');
                    }}, 300);  // Match the transition duration from CSS
                }} else {{
                    navPane.style.opacity = '0.1';
                    navPane.style.pointerEvents = 'none';
                }}
            }}

            function toggleNav(e) {{
                e.stopPropagation();
                if (navPane.classList.contains('visible')) {{
                    hideNav();
                }} else {{
                    showNav();
                }}
            }}

            // Mobile-specific handlers
            if (isMobile()) {{
                navTrigger.addEventListener('click', (e) => {{
                    e.stopPropagation();
                    if (navPane.classList.contains('visible')) {{
                        hideNav();
                    }} else {{
                        navPane.classList.add('visible');
                        navPane.style.opacity = '1';
                    }}
                }});
                
                // Close menu when clicking outside
                document.addEventListener('click', (e) => {{
                    if (!navPane.contains(e.target) && !navTrigger.contains(e.target)) {{
                        hideNav();
                    }}
                }});
            }} else {{
                // Desktop hover behavior
                navTrigger.addEventListener('mouseenter', showNav);
                navPane.addEventListener('mouseenter', showNav);
                navPane.addEventListener('mouseleave', hideNav);
                navTrigger.addEventListener('mouseleave', hideNav);
            }}

            // Handle resize events
            window.addEventListener('resize', () => {{
                if (!isMobile()) {{
                    navPane.classList.remove('visible');
                    navPane.style.transform = 'none';
                }}
            }});
            
            // Initial state
            if (isMobile()) {{
                hideNav();
            }} else {{
                setTimeout(hideNav, 1000);
            }}
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
