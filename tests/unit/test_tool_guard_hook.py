import importlib.util
import io
import json
from contextlib import redirect_stdout
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
_HOOK_PATH = _ROOT / "ci" / "hooks" / "tool_guard.py"
_HOOK_COMMAND = "ci/hooks/tool_guard"
_SPEC = importlib.util.spec_from_file_location("tool_guard", _HOOK_PATH)
assert _SPEC is not None
assert _SPEC.loader is not None
tool_guard = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(tool_guard)


def _run_hook(payload: dict[str, object]) -> str:
    original_stdin = tool_guard.sys.stdin
    stdout = io.StringIO()
    try:
        tool_guard.sys.stdin = io.StringIO(json.dumps(payload))
        with redirect_stdout(stdout):
            assert tool_guard.main() == 0
    finally:
        tool_guard.sys.stdin = original_stdin
    return stdout.getvalue()


def test_hook_handles_claude_bash_payload() -> None:
    output = _run_hook({"tool_name": "Bash", "tool_input": {"command": "cargo test"}})
    assert "permissionDecision" in output
    assert "soldr cargo" in output


def test_hook_handles_codex_shell_payload() -> None:
    output = _run_hook(
        {
            "tool_name": "functions.shell_command",
            "tool_input": {"command": "cargo test"},
        }
    )
    assert "permissionDecision" in output
    assert "soldr cargo" in output


def test_hook_allows_soldr() -> None:
    output = _run_hook(
        {
            "tool_name": "shell_command",
            "tool_input": {"command": "soldr cargo test --workspace"},
        }
    )
    assert output == ""


def test_hook_configs_use_cross_platform_wrapper() -> None:
    config_paths = [
        _ROOT / ".claude" / "settings.json",
        _ROOT / ".codex" / "hooks.json",
    ]

    for path in config_paths:
        data = json.loads(path.read_text(encoding="utf-8"))
        for entries in data["hooks"].values():
            for entry in entries:
                for hook in entry["hooks"]:
                    assert hook["command"] == _HOOK_COMMAND


def test_hook_wrappers_select_platform_python() -> None:
    posix_wrapper = (_ROOT / "ci" / "hooks" / "tool_guard").read_text(encoding="utf-8")
    windows_wrapper = (_ROOT / "ci" / "hooks" / "tool_guard.cmd").read_text(
        encoding="utf-8"
    )

    assert "MINGW*|MSYS*|CYGWIN*" in posix_wrapper
    assert "python ci/hooks/tool_guard.py" in posix_wrapper
    assert "python3 ci/hooks/tool_guard.py" in posix_wrapper
    assert "python ci\\hooks\\tool_guard.py" in windows_wrapper
    assert "uv run" not in posix_wrapper
    assert "uv run" not in windows_wrapper
