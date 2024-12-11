from fastled import WebCompileResult


def test_examples(
    examples: list[str] | None = None, host: str | None = None
) -> Exception | None:
    """Test the examples in the given directory."""
    from fastled import Api

    examples = Api.get_examples() if examples is None else examples
    for example in examples:
        print(f"Compiling example: {example}")
        out: WebCompileResult = Api.web_compile(example, host=host)
        if not out.success:
            return Exception(out.stdout)
    return None
