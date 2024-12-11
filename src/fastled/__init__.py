"""FastLED Wasm Compiler package."""

__version__ = "1.1.35"

# from .compile_server import CompileServer


# from .project_init import get_examples, project_init
from .web_compile import web_compile

# __all__ = ["CompileServer", "web_compile", "project_init", "get_examples"]

__all__ = ["web_compile"]
