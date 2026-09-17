#!/usr/bin/env python3
"""Fail when a viewer screenshot is not a real render (#247).

The wheel smoke used to check only the PNG header and dimensions, so a blank
canvas passed: exactly the failure mode of #247 (the viewer loads, the WebGL2
compatibility gate fails, nothing is drawn) and of #250 (no screen map reaches
the renderer). This decodes the image and requires a minimum fraction of lit
pixels.

Runs under the wheel's own venv interpreter, which has no third-party
packages, so the PNG decoding here is stdlib-only.
"""

from __future__ import annotations

import argparse
import sys
import zlib
from pathlib import Path

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
# Channel value above which a subpixel counts as lit. The viewer renders LEDs
# on a near-black background, so anything clearly above black counts.
LIT_THRESHOLD = 60


def _chunks(data: bytes):
    offset = len(PNG_SIGNATURE)
    while offset + 8 <= len(data):
        length = int.from_bytes(data[offset : offset + 4], "big")
        kind = data[offset + 4 : offset + 8]
        body = data[offset + 8 : offset + 8 + length]
        yield kind, body
        offset += 12 + length  # length + type + body + CRC


def _paeth(a: int, b: int, c: int) -> int:
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def lit_fraction(png: bytes) -> tuple[float, int, int]:
    """Return (lit fraction, lit pixels, total pixels) for an 8-bit PNG."""
    if png[: len(PNG_SIGNATURE)] != PNG_SIGNATURE:
        raise SystemExit("screenshot is not a PNG")

    header = b""
    pixels = b""
    for kind, body in _chunks(png):
        if kind == b"IHDR":
            header = body
        elif kind == b"IDAT":
            pixels += body
        elif kind == b"IEND":
            break

    if len(header) < 13:
        raise SystemExit("screenshot is missing its IHDR header")
    width = int.from_bytes(header[0:4], "big")
    height = int.from_bytes(header[4:8], "big")
    depth = header[8]
    color_type = header[9]
    interlace = header[12]
    if width <= 0 or height <= 0:
        raise SystemExit(f"screenshot has invalid dimensions: {width}x{height}")
    # The viewer writes 8-bit RGB/RGBA, non-interlaced. Anything else is a
    # change worth failing on rather than guessing at.
    if depth != 8 or color_type not in (2, 6) or interlace != 0:
        raise SystemExit(
            f"unsupported PNG: depth={depth} color_type={color_type} interlace={interlace}"
        )

    channels = 3 if color_type == 2 else 4
    stride = width * channels
    raw = zlib.decompress(pixels)
    if len(raw) < height * (stride + 1):
        raise SystemExit("screenshot pixel data is truncated")

    lit = 0
    previous = bytearray(stride)
    offset = 0
    for _ in range(height):
        filter_type = raw[offset]
        line = bytearray(raw[offset + 1 : offset + 1 + stride])
        offset += 1 + stride
        for index in range(stride):
            left = line[index - channels] if index >= channels else 0
            up = previous[index]
            up_left = previous[index - channels] if index >= channels else 0
            if filter_type == 1:
                line[index] = (line[index] + left) & 0xFF
            elif filter_type == 2:
                line[index] = (line[index] + up) & 0xFF
            elif filter_type == 3:
                line[index] = (line[index] + ((left + up) >> 1)) & 0xFF
            elif filter_type == 4:
                line[index] = (line[index] + _paeth(left, up, up_left)) & 0xFF
            elif filter_type != 0:
                raise SystemExit(f"unsupported PNG row filter: {filter_type}")
        for index in range(0, stride, channels):
            if max(line[index : index + 3]) > LIT_THRESHOLD:
                lit += 1
        previous = line

    total = width * height
    return lit / total, lit, total


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("screenshot", type=Path)
    parser.add_argument(
        "--min-lit-fraction",
        type=float,
        default=0.005,
        help="minimum fraction of pixels that must be lit (default: 0.005)",
    )
    args = parser.parse_args()

    fraction, lit, total = lit_fraction(args.screenshot.read_bytes())
    print(f"{args.screenshot.name}: {lit}/{total} pixels lit ({fraction:.4f})")
    if fraction < args.min_lit_fraction:
        print(
            f"{args.screenshot.name} is blank: {fraction:.4f} lit, "
            f"need {args.min_lit_fraction}. The viewer loaded but drew nothing.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
