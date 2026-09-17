"""Refs #247: a blank viewer screenshot must fail the wheel smoke."""

import struct
import subprocess
import sys
import zlib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "ci/check_render.py"


def _png(pixels: list[list[tuple[int, int, int]]], filter_type: int = 0) -> bytes:
    """Encode 8-bit RGB rows, optionally with a per-row PNG filter applied."""
    width = len(pixels[0])
    raw = b""
    previous = bytearray(width * 3)
    for row in pixels:
        line = bytearray()
        for red, green, blue in row:
            line += bytes((red, green, blue))
        if filter_type == 0:
            encoded = bytes(line)
        elif filter_type == 2:  # Up
            encoded = bytes((line[i] - previous[i]) & 0xFF for i in range(len(line)))
        else:
            raise AssertionError(f"unsupported test filter {filter_type}")
        raw += bytes((filter_type,)) + encoded
        previous = line

    def chunk(kind: bytes, body: bytes) -> bytes:
        payload = kind + body
        return (
            struct.pack(">I", len(body))
            + payload
            + struct.pack(">I", zlib.crc32(payload))
        )

    header = struct.pack(">IIBBBBB", width, len(pixels), 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def _run(path: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(CHECKER), str(path), *args],
        capture_output=True,
        text=True,
        check=False,
    )


def test_blank_screenshot_fails(tmp_path: Path) -> None:
    """RED for #247/#250: the viewer loaded but drew nothing."""
    shot = tmp_path / "blank.png"
    shot.write_bytes(_png([[(0, 0, 0)] * 8 for _ in range(8)]))
    result = _run(shot)
    assert result.returncode == 1, result.stdout
    assert "is blank" in result.stderr


def test_dim_screenshot_below_threshold_fails(tmp_path: Path) -> None:
    """Background noise must not count as a render."""
    shot = tmp_path / "dim.png"
    shot.write_bytes(_png([[(40, 20, 10)] * 8 for _ in range(8)]))
    assert _run(shot).returncode == 1


def test_rendered_screenshot_passes(tmp_path: Path) -> None:
    rows = [[(0, 0, 0)] * 8 for _ in range(8)]
    for x in range(8):
        rows[3][x] = (255, 0, 0)
    shot = tmp_path / "lit.png"
    shot.write_bytes(_png(rows))
    result = _run(shot)
    assert result.returncode == 0, result.stderr
    assert "8/64 pixels lit" in result.stdout


def test_filtered_rows_are_decoded(tmp_path: Path) -> None:
    """Real viewer screenshots use row filters; decoding must honour them."""
    rows = [[(255, 255, 255)] * 4 for _ in range(4)]
    shot = tmp_path / "filtered.png"
    shot.write_bytes(_png(rows, filter_type=2))
    result = _run(shot)
    assert result.returncode == 0, result.stderr
    assert "16/16 pixels lit" in result.stdout


def test_threshold_is_configurable(tmp_path: Path) -> None:
    rows = [[(0, 0, 0)] * 10 for _ in range(10)]
    rows[0][0] = (255, 255, 255)
    shot = tmp_path / "sparse.png"
    shot.write_bytes(_png(rows))
    assert _run(shot, "--min-lit-fraction", "0.005").returncode == 0
    assert _run(shot, "--min-lit-fraction", "0.5").returncode == 1


@pytest.mark.parametrize("payload", [b"not a png", b"\x89PNG\r\n\x1a\n"])
def test_non_png_input_fails(tmp_path: Path, payload: bytes) -> None:
    shot = tmp_path / "bad.png"
    shot.write_bytes(payload)
    assert _run(shot).returncode != 0
