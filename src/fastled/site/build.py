import argparse
import subprocess
from pathlib import Path
from shutil import copytree, rmtree, which

CSS_CONTENT = """
/* CSS Reset & Variables */
*, *::before, *::after {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
}

:root {
    --color-background: #121212;
    --color-surface: #252525;
    --color-surface-transparent: rgba(30, 30, 30, 0.95);
    --color-text: #E0E0E0;
    --spacing-sm: 5px;
    --spacing-md: 10px;
    --spacing-lg: 15px;
    --transition-speed: 0.3s;
    --font-family: 'Roboto Condensed', sans-serif;
    --nav-width: 250px;
    --border-radius: 5px;
}

/* Base Styles */
body {
    background-color: var(--color-background);
    color: var(--color-text);
    margin: 0;
    padding: 0;
    font-family: var(--font-family);
    min-height: 100vh;
    display: grid;
    grid-template-rows: 1fr;
}

/* Splash Screen */
.splash-screen {
    position: fixed;
    inset: 0;
    background-color: var(--color-background);
    display: flex;
    justify-content: center;
    align-items: center;
    z-index: 2000;
    transition: opacity var(--transition-speed) ease-out;
}

.splash-text {
    font-size: 14vw;
    color: var(--color-text);
    font-weight: 300;
    font-family: var(--font-family);
    opacity: 0;
    transition: opacity var(--transition-speed) ease-in;
}

/* Layout */
.content-wrapper {
    position: relative;
    width: 100%;
    height: 100vh;
    overflow-x: hidden;
}

/* Navigation */
.nav-trigger {
    position: fixed;
    left: var(--spacing-md);
    top: var(--spacing-md);
    padding: var(--spacing-sm) var(--spacing-lg);
    z-index: 1001;
    background-color: var(--color-surface);
    border-radius: var(--border-radius);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    color: var(--color-text);
    font-size: 16px;
    transition: background-color var(--transition-speed) ease;
}

.nav-trigger:hover {
    background-color: var(--color-surface-transparent);
}

.nav-pane {
    position: fixed;
    left: var(--spacing-md);
    top: 60px;
    width: var(--nav-width);
    height: auto;
    background-color: var(--color-surface-transparent);
    border-radius: var(--border-radius);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.5);
    transform: translateY(-20px);
    opacity: 0;
    pointer-events: none;
    transition: transform var(--transition-speed) ease,
                opacity var(--transition-speed) ease;
}

.nav-pane.visible {
    transform: translateY(0);
    opacity: 1;
    pointer-events: auto;
}

/* Main Content */
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
    background-color: var(--color-background);
    overflow: auto;
}

/* Example Links */
.example-link {
    margin: var(--spacing-sm) var(--spacing-md);
    padding: var(--spacing-lg) var(--spacing-md);
    border-radius: var(--border-radius);
    display: block;
    text-decoration: none;
    color: var(--color-text);
    background-color: var(--color-surface);
    transition: background-color var(--transition-speed) ease-in-out,
                box-shadow var(--transition-speed) ease-in-out;
    position: relative;
    padding-right: 35px;
}

.example-link:hover {
    background-color: var(--color-surface-transparent);
    box-shadow: var(--shadow-hover, 0 0 10px rgba(255, 255, 255, 0.1));
}

.example-link:last-child {
    margin-bottom: var(--spacing-md);
}

/* Accessibility */
@media (prefers-reduced-motion: reduce) {
    *, *::before, *::after {
        animation-duration: 0.01ms !important;
        animation-iteration-count: 1 !important;
        transition-duration: 0.01ms !important;
        scroll-behavior: auto !important;
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
    <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.4.0/css/all.min.css">
    <link rel="stylesheet" href="index.css">
</head>
<body>
    <div class="splash-screen">
        <div class="splash-text">FastLED</div>
    </div>
    <div class="content-wrapper">
        <div class="nav-trigger">Examples</div>
        <nav class="nav-pane">
            {example_links}
        </nav>
        <main class="main-content">
            <iframe id="example-frame" title="Example Content"></iframe>
        </main>
    </div>
    <script>
        document.addEventListener('DOMContentLoaded', function() {{
            const splashScreen = document.querySelector('.splash-screen');
            const splashText = document.querySelector('.splash-text');
            
            // Wait for font to load
            document.fonts.ready.then(() => {{
                // Fade in the text
                splashText.style.opacity = '1';
                
                // Wait for page load plus fade-in time before starting fade-out sequence
                window.addEventListener('load', () => {{
                    setTimeout(() => {{
                        splashScreen.style.opacity = '0';
                        setTimeout(() => {{
                            splashScreen.style.display = 'none';
                        }}, 500); // Remove from DOM after fade completes
                    }}, 1500); // Wait for load + 1.5s (giving time for fade-in)
                }});
            }});
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
                checkmark.style.color = '#E0E0E0';
                link.appendChild(checkmark);
            }});
            
            // Now load first example and show its checkmark
            if (links.length > 0) {{
                // Try to find SdCard example first
                let startLink = Array.from(links).find(link => link.textContent === 'SdCard') || links[0];
                iframe.src = startLink.getAttribute('href');
                startLink.classList.add('active');
                startLink.querySelector('.fa-check').style.display = 'inline-block';
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


EXAMPLES = [
    "wasm",
    "Chromancer",
    "LuminescentGrand",
    "FxSdCard",
    "FxNoiseRing",
    "FxWater",
]


def _exec(cmd: str) -> None:
    subprocess.run(cmd, shell=True, check=True)


def build_example(example: str, outputdir: Path) -> None:
    if not which("fastled"):
        raise FileNotFoundError("fastled executable not found")
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


def generate_css(outputdir: Path) -> None:
    css_file = outputdir / "index.css"
    # with open(css_file, "w") as f:
    #    f.write(CSS_CONTENT, encoding="utf-8")
    css_file.write_text(CSS_CONTENT, encoding="utf-8")


def build_index_html(outputdir: Path) -> None:
    outputdir = outputdir
    assert (
        outputdir.exists()
    ), f"Output directory {outputdir} not found, you should run build_example first"
    index_html = outputdir / "index.html"

    examples = [f for f in outputdir.iterdir() if f.is_dir()]
    examples = sorted(examples)

    example_links = "\n".join(
        f'    <a class="example-link" href="{example.name}/index.html">{example.name}</a>'
        for example in examples
    )

    with open(index_html, "w") as f:
        f.write(INDEX_TEMPLATE.format(example_links=example_links))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build FastLED example site")
    parser.add_argument(
        "--outputdir", type=Path, help="Output directory", required=True
    )
    parser.add_argument(
        "--fast",
        action="store_true",
        help="Skip regenerating existing examples, only rebuild index.html and CSS",
    )
    return parser.parse_args()


def build(outputdir: Path, fast: bool | None = None, check=False) -> list[Exception]:
    outputdir = outputdir
    fast = fast or False
    errors: list[Exception] = []

    for example in EXAMPLES:
        example_dir = outputdir / example
        if not fast or not example_dir.exists():
            try:
                build_example(example=example, outputdir=outputdir)
            except Exception as e:
                if check:
                    raise
                errors.append(e)

    try:
        generate_css(outputdir=outputdir)
    except Exception as e:
        if check:
            raise
        errors.append(e)

    try:
        build_index_html(outputdir=outputdir)
    except Exception as e:
        if check:
            raise
        errors.append(e)

    return errors


def main() -> int:
    args = parse_args()
    outputdir = args.outputdir
    fast = args.fast
    build(outputdir=outputdir, fast=fast)
    return 0


if __name__ == "__main__":
    import sys

    sys.argv.append("--fast")
    main()
