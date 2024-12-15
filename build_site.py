import argparse

from pathlib import Path
from fastled.site.build import build

HERE = Path(__file__).parent.resolve()
SITE = HERE / "site"

def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description='Build FastLED example site')
    parser.add_argument('--fast', action='store_true', 
                       help='Skip regenerating existing examples, only rebuild index.html and CSS')
    return parser.parse_args()

def main() -> None:
    args = parse_args()
    build(SITE, fast=args.fast)

if __name__ == "__main__":
    main()