# UV Usage Guide - FastLED WASM Project

## 🚨 CRITICAL REQUIREMENT: Use `uv` for Python Execution

This project **MUST** use `uv` instead of `python` or `python3` for all Python code execution.

## Files Updated

### 1. `.cursorrules`
- Added comprehensive rules emphasizing `uv run` usage
- Includes examples, rationale, and development workflow
- Provides clear do's and don'ts for Python execution

### 2. `.mcp-server-config.json`
- MCP server configuration with strict enforcement rules
- JSON structure defining correct and incorrect usage patterns
- Includes exceptions for legitimate `python` usage (imports, configs, etc.)

### 3. `.vscode/settings.json`
- Updated Python interpreter path to use `uv`
- Added terminal launch args to automatically use `uv run`
- Added comment emphasizing the critical requirement

## Quick Reference

### ✅ CORRECT Usage:
```bash
uv run src/fastled/cli.py
uv run -m pytest
uv run build_exe.py
uv run python script.py  # if explicitly needed
```

### ❌ INCORRECT Usage:
```bash
python src/fastled/cli.py      # DON'T DO THIS
python3 build_exe.py           # DON'T DO THIS
python -m pytest              # DON'T DO THIS
```

## Why Use `uv`?

1. **Consistent Environment**: Ensures correct Python version and dependencies
2. **Package Management**: Integrated with project's dependency management
3. **Virtual Environment**: Automatically managed by `uv`
4. **Version Control**: Specified in `pyproject.toml` and lock files

## Testing

- Unit tests: Use `bash test` (as per user rules)
- Manual testing: Prefix Python commands with `uv run`

## Development Workflow

1. Install dependencies: `uv add <package>`
2. Run scripts: `uv run <script.py>`
3. Run modules: `uv run -m <module>`
4. Environment is automatically managed by `uv`

## Enforcement

- **Level**: Strict
- **Actions**: Warn, suggest alternatives, update scripts
- **Scope**: All Python execution commands

## Exceptions

`python` usage is only allowed in:
- Python source code (imports, etc.)
- Configuration files specifying Python version
- Dockerfiles where `uv` is not available
- CI/CD where setup-python action is used

---

**Remember**: When in doubt, prefix with `uv run`! 🚀