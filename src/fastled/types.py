from dataclasses import dataclass
from typing import Any


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
