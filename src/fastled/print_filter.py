import re
import zlib
from abc import ABC, abstractmethod
from dataclasses import dataclass
from enum import Enum


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


class ChunkedBuildConfigGrouper:
    """
    Groups compiler invocations by identical flag-sets.
    Yields:
      - A blank line + the flags when they change
      - Then one line per file: "[time] filename"
    """

    _line_re = re.compile(
        r"^\s*(?P<time>\d+\.\d+)\s+"
        r"(?P<flags>.+?)\s+"
        r"(?P<file>[^ ]+\.(?:cpp|c|ino))\s*$"
    )

    def __init__(self) -> None:
        self._prev_key: str | None = None

    def filter(self, text: str) -> str:
        out: list[str] = []
        for raw in text.splitlines(keepends=True):
            m = self._line_re.match(raw)
            if not m:
                # passthrough anything that doesn’t match
                out.append(raw)
                continue

            time_stamp = m.group("time")
            flags = m.group("flags")
            src_file = m.group("file")

            # on a new flag-set, emit a blank line + the flags
            if flags != self._prev_key:
                self._prev_key = flags
                # only emit a blank line if output is non-empty and last line isn't blank
                if out and not out[-1].isspace():
                    out.append("\n")
                out.append(flags + "\n")

            # then emit the timestamp + filename
            out.append(f"{time_stamp} {src_file}\n")

        return "".join(out)


class CompileOrLink(Enum):
    COMPILE = "compile"
    LINK = "link"


@dataclass
class BuildArtifact:
    timestamp: float
    output_artifact: str | None
    build_flags: str
    compile_or_link: CompileOrLink
    hash: int

    def __str__(self) -> str:
        return f"{self.timestamp} {self.output_artifact} {self.build_flags} {self.compile_or_link} {self.hash}"

    @staticmethod
    def parse(input_str: str) -> "BuildArtifact | None":
        """
        Parse a single build-log line of the form:
          "<timestamp> ... <some .cpp or .h file> ... <flags>"

        Returns a BuildArtifact, or None if parsing failed.
        """
        return _parse(input_str)


def _parse(input_str: str) -> BuildArtifact | None:
    """
    Parse a single build-log line of the form:
      "<timestamp> ... <some .cpp or .h file> ... <flags>"

    Returns a BuildArtifact, or None if parsing failed.
    """
    parts = input_str.strip().split()
    if len(parts) < 2:
        return None

    # 1) timestamp
    try:
        ts = float(parts[0])
    except ValueError:
        return None

    # 2) find the first .cpp or .h token
    file_tok = ""
    file_idx = None
    for i, tok in enumerate(parts):
        if tok.endswith(".cpp") or tok.endswith(".h"):
            file_idx = i
            file_tok = tok
            break
    if file_idx is None:
        return None

    # 3) build_flags = everything except parts[0] and that file token
    #    (remove only the one occurrence)
    flags_tokens = parts[1:]
    # remove the file token at the adjusted index
    rel_idx = file_idx - 1
    if 0 <= rel_idx < len(flags_tokens):
        del flags_tokens[rel_idx]
    flags_str = " ".join(flags_tokens)

    # 4) decide compile vs. link
    action = CompileOrLink.COMPILE if "-c" in flags_tokens else CompileOrLink.LINK

    # 5) stable integer hash of the flags
    h = zlib.adler32(flags_str.encode("utf-8"))

    return BuildArtifact(
        timestamp=ts,
        output_artifact=file_tok,
        build_flags=flags_str,
        compile_or_link=action,
        hash=h,
    )
