from tempfile import TemporaryDirectory
from time import time
from warnings import warn

_FILTER = True


def test_examples(
    examples: list[str] | None = None, host: str | None = None
) -> dict[str, Exception]:
    """Test the examples in the given directory."""
    from fastled import Api

    out: dict[str, Exception] = {}
    examples = Api.get_examples(host=host) if examples is None else examples
    if host is None and _FILTER:
        examples.remove("Chromancer")  # Brutal
        examples.remove("LuminescentGrand")
    with TemporaryDirectory() as tmpdir:
        for example in examples:
            print(f"Initializing example: {example}")
            try:
                sketch_dir = Api.project_init(example, outputdir=tmpdir, host=host)
            except Exception as e:
                warn(f"Failed to initialize example: {example}, error: {e}")
                out[example] = e
                continue
            print(f"Project initialized at: {sketch_dir}")
            start = time()
            print(f"Compiling example: {example}")
            diff = time() - start
            print(f"Compilation took: {diff:.2f} seconds")
            result = Api.web_compile(sketch_dir, host=host)
            if not result.success:
                out[example] = Exception(result.stdout)
    return out


def unit_test() -> None:
    from fastled import Api

    with Api.server(auto_updates=True) as server:
        out = test_examples(host=server.url())
        if out:
            raise RuntimeError(f"Failed tests: {out}")


if __name__ == "__main__":
    unit_test()
