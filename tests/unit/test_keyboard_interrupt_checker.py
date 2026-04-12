import importlib.util
import sys
from pathlib import Path

_CHECKER_PATH = (
    Path(__file__).resolve().parents[2]
    / "ci"
    / "lint_python"
    / "keyboard_interrupt_checker.py"
)
_SPEC = importlib.util.spec_from_file_location(
    "keyboard_interrupt_checker", _CHECKER_PATH
)
assert _SPEC is not None
assert _SPEC.loader is not None
_MODULE = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = _MODULE
_SPEC.loader.exec_module(_MODULE)

check_file = _MODULE.check_file
has_broad_except = _MODULE.has_broad_except


def codes(code: str) -> list[str]:
    return [violation.code for violation in check_file("<test>", code)]


def test_regex_prefilter_matches_broad_handlers() -> None:
    assert has_broad_except("try:\n    pass\nexcept:\n    pass")
    assert has_broad_except("except Exception as exc:\n    pass")
    assert has_broad_except("except (ValueError, KeyboardInterrupt):\n    pass")
    assert not has_broad_except("except ValueError:\n    pass")


def test_kbi001_for_broad_except_without_interrupt_handler() -> None:
    code = """\
try:
    pass
except Exception:
    pass
"""
    assert "KBI001" in codes(code)


def test_kbi002_for_keyboard_interrupt_handler_without_notification() -> None:
    code = """\
try:
    pass
except KeyboardInterrupt:
    raise
"""
    assert "KBI002" in codes(code)


def test_kbi003_for_helper_call_outside_keyboard_interrupt_handler() -> None:
    code = """\
try:
    handle_keyboard_interrupt(None)
except Exception:
    pass
"""
    assert "KBI003" in codes(code)


def test_good_keyboard_interrupt_pattern_is_accepted() -> None:
    code = """\
try:
    pass
except KeyboardInterrupt as ki:
    handle_keyboard_interrupt(ki)
except Exception:
    pass
"""
    assert check_file("<test>", code) == []


def test_noqa_suppresses_specific_violation() -> None:
    code = """\
try:  # noqa: KBI001
    pass
except Exception:
    pass
"""
    assert check_file("<test>", code) == []


def test_baseline_loader_ignores_comments_and_blank_lines(tmp_path: Path) -> None:
    baseline = tmp_path / "kbi-baseline.txt"
    baseline.write_text("\n# comment\nfoo\n\nbar\n", encoding="utf-8")

    assert _MODULE._load_baseline(str(baseline)) == {"foo", "bar"}
