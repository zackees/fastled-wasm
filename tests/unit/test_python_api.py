from fastled import (
    BuildArtifacts,
    BuildMode,
    BuildRequest,
    BuildService,
    CompileResult,
    __version__,
    is_in_order_match,
    string_diff,
)


def test_python_package_exports_native_api() -> None:
    assert __version__
    for symbol in (
        BuildArtifacts,
        BuildMode,
        BuildRequest,
        BuildService,
        CompileResult,
        is_in_order_match,
        string_diff,
    ):
        assert symbol is not None
