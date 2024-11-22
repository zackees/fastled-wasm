from dataclasses import dataclass


@dataclass
class CompiledResult:
    """Dataclass to hold the result of the compilation."""

    success: bool
    fastled_js: str
    hash_value: str | None
