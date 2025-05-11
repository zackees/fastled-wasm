import re
from abc import ABC, abstractmethod


class PrintFilter(ABC):
    """Abstract base class for filtering text output."""

    def __init__(self, echo: bool = True) -> None:
        self.echo = echo

    @abstractmethod
    def filter(self, text: str) -> str:
        """Filter the text according to implementation-specific rules."""
        pass

    def print(self, text: str | bytes) -> str:
        """Prints the text to the console after filtering."""
        if isinstance(text, bytes):
            text = text.decode("utf-8")
        text = self.filter(text)
        if self.echo:
            print(text, end="")
        return text


def _handle_ino_cpp(line: str) -> str:
    if ".ino.cpp" in line[0:30]:
        # Extract the filename without path and extension
        match = re.search(r"src/([^/]+)\.ino\.cpp", line)
        if match:
            filename = match.group(1)
            # Replace with examples/Filename/Filename.ino format
            line = line.replace(
                f"src/{filename}.ino.cpp", f"examples/{filename}/{filename}.ino"
            )
        else:
            # Fall back to simple extension replacement if regex doesn't match
            line = line.replace(".ino.cpp", ".ino")
    return line


def _handle_fastled_src(line: str) -> str:
    return line.replace("fastled/src", "src")


class PrintFilterDefault(PrintFilter):
    """Provides default filtering for FastLED output."""

    def filter(self, text: str) -> str:
        return text


class PrintFilterFastled(PrintFilter):
    """Provides filtering for FastLED output so that source files match up with local names."""

    def __init__(self, echo: bool = True) -> None:
        super().__init__(echo)
        self.build_started = False

    def filter(self, text: str) -> str:
        lines = text.splitlines()
        out: list[str] = []
        for line in lines:
            ## DEBUG DO NOT SUBMIT
            # print(line)
            if "# WASM is building" in line:
                self.build_started = True
            line = _handle_fastled_src(
                line
            )  # Always convert fastled/src to src for file matchups.
            if self.build_started or " error: " in line:
                line = _handle_ino_cpp(line)
            out.append(line)
        text = "\n".join(out)
        return text
