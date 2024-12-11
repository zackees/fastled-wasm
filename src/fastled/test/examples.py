from tempfile import TemporaryDirectory


def test_examples(
    examples: list[str] | None = None, host: str | None = None
) -> dict[str, Exception]:
    """Test the examples in the given directory."""
    from fastled import Api

    out: dict[str, Exception] = {}
    examples = Api.get_examples() if examples is None else examples
    with TemporaryDirectory() as tmpdir:
        for example in examples:
            print(f"Initializing example: {example}")
            sketch_dir = Api.project_init(example, outputdir=tmpdir, host=host)
            print(f"Project initialized at: {sketch_dir}")
            print(f"Compiling example: {example}")
            result = Api.web_compile(sketch_dir, host=host)
            if not result.success:
                out[example] = Exception(result.stdout)
    return out


def unit_test() -> None:
    out = test_examples()
    if out:
        raise RuntimeError(f"Failed tests: {out}")


if __name__ == "__main__":
    unit_test()
