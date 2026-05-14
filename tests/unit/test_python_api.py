import importlib.util

import fastled


def test_python_package_is_cli_only() -> None:
    assert fastled.__version__
    assert fastled.__all__ == ["__version__"]
    assert not hasattr(fastled, "BuildService")


def test_native_extension_is_not_packaged() -> None:
    assert importlib.util.find_spec("fastled._native") is None
