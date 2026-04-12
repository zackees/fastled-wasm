import subprocess
import sys
from pathlib import Path

SRC_DIR = str(Path(__file__).parent.parent.parent / "src")


def _run_fastled(*args: str) -> subprocess.CompletedProcess[str]:
    import os

    env = os.environ.copy()
    env["PYTHONPATH"] = SRC_DIR + os.pathsep + env.get("PYTHONPATH", "")
    return subprocess.run(
        [sys.executable, "-m", "fastled", *args],
        capture_output=True,
        text=True,
        timeout=60,
        stdin=subprocess.DEVNULL,
        env=env,
    )


def test_help_no_longer_advertises_local_flag() -> None:
    result = _run_fastled("--help")
    legacy_flag = "--" + "local"
    assert legacy_flag not in result.stdout


def test_help_no_longer_advertises_server_or_web_flags() -> None:
    result = _run_fastled("--help")
    assert "--server" not in result.stdout
    assert "--web" not in result.stdout


def test_parse_args_source_has_no_local_flag() -> None:
    source = (
        Path(__file__).parent.parent.parent / "src" / "fastled" / "parse_args.py"
    ).read_text()
    legacy_flag = "--" + "local"
    assert legacy_flag not in source


def test_parse_args_source_has_no_server_or_web_flags() -> None:
    source = (
        Path(__file__).parent.parent.parent / "src" / "fastled" / "parse_args.py"
    ).read_text()
    assert "--server" not in source
    assert "--web" not in source


def test_help_advertises_serve_dir() -> None:
    result = _run_fastled("--help")
    assert "--serve-dir" in result.stdout
