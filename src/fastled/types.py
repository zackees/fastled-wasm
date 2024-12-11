from dataclasses import dataclass
from typing import Any


@dataclass
class InternalCompiledResult:  # not mean to be used outside the api.
    """Dataclass to hold the result of the compilation."""

    success: bool
    fastled_js: str
    hash_value: str | None


@dataclass
class WebCompileResult:
    success: bool
    stdout: str
    hash_value: str | None
    zip_bytes: bytes

    def __bool__(self) -> bool:
        return self.success

    def to_dict(self) -> dict[str, Any]:
        return self.__dict__.copy()


class CompileServerError(Exception):
    """Error class for failing to instantiate CompileServer."""

    pass
