import argparse

from pathlib import Path
from fastled import Test

HERE = Path(__file__).parent.resolve()
SITE = HERE / "site"

def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description='Build FastLED example site')
    parser.add_argument('--fast', action='store_true', 
                       help='Skip regenerating existing examples, only rebuild index.html and CSS')
    return parser.parse_args()

def main() -> None:
    args = parse_args()
    Test.build_site(SITE, fast=args.fast, check=True)

if __name__ == "__main__":
    main()