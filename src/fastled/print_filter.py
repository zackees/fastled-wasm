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
        # self.compile_link_active = False
        # self.compile_link_filter:

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


class CompileOrLink(Enum):
    COMPILE = "compile"
    LINK = "link"


@dataclass
class BuildArtifact:
    timestamp: float
    input_artifact: str | None
    output_artifact: str | None
    build_flags: str
    compile_or_link: CompileOrLink
    hash: int

    def flags_pretty(self) -> str:
        """
        Returns the flags in a pretty format.
        This is used for printing the flags to the console.
        """
        flags = self.build_flags
        flags = flags.replace(" -I", "\n-I")
        flags = flags.replace(" -D", "\n-D")
        flags = flags.replace(" -l", "\n-l")
        flags = flags.replace(" -L", "\n-L")
        flags = flags.replace(" -o", "\n-o")
        flags = flags.replace(" -W", "\n-W")
        flags = flags.replace(" -f", "\n-f")
        flags = flags.replace(" -g", "\n-g")

        # break into lines and sort
        lines = flags.splitlines()
        first_line = lines[0]
        lines.pop(0)  # remove first line
        lines = sorted(lines)
        # remove duplicates
        lines = list(dict.fromkeys(lines))
        # remove empty lines
        lines = [line for line in lines if line.strip() != ""]
        # remove leading and trailing whitespace
        lines = [line.strip() for line in lines]
        lines = sorted(lines)
        lines = [first_line] + lines  # add first line back to the beginning
        # stringify
        flags = "\n".join(lines)
        return flags

    def __str__(self) -> str:
        return f"{self.brief()} {self.build_flags} {self.compile_or_link} {self.hash}"

    def brief(self) -> str:
        return f"{self.timestamp:.2f} {self.output_artifact}"

    def begin_flags(self) -> str:
        """
        Returns the flags that are used to begin a build.
        This is the flags that are used for the first compile or link.
        """

        out: str = (
            "\n################ NEW COMPILE/LINK FLAG GROUP #####################\n\n"
        )
        out += f"{self.flags_pretty()}\n"
        return out

    def end_flags(self) -> str:
        """
        Returns the flags that are used to end a build.
        This is the flags that are used for the last compile or link.
        """
        out: str = (
            "\n################ END COMPILE/LINK FLAG GROUP #####################\n"
        )
        return out

    @staticmethod
    def parse(input_str: str) -> "BuildArtifact | None":
        """
        Parse a single build-log line of the form:
          "<timestamp> ... <some .cpp or .h file> ... <flags>"

        Returns a BuildArtifact, or None if parsing failed.
        """
        return _parse(input_str)


class TokenFilter(ABC):
    @abstractmethod
    def extract(self, tokens: list[str]) -> str | None:
        """
        Scan `tokens`, remove any tokens this filter is responsible for,
        and return the extracted string (or None if not found/invalid).
        """
        ...


class TimestampFilter(TokenFilter):
    def extract(self, tokens: list[str]) -> str | None:
        if not tokens:
            return None
        candidate = tokens[0]
        try:
            _ = float(candidate)
            return tokens.pop(0)
        except ValueError:
            return None


class InputArtifactFilter(TokenFilter):
    def extract(self, tokens: list[str]) -> str | None:
        for i, tok in enumerate(tokens):
            if tok.endswith(".cpp") or tok.endswith(".h"):
                return tokens.pop(i)
        return None


class OutputArtifactFilter(TokenFilter):
    def extract(self, tokens: list[str]) -> str | None:
        for i, tok in enumerate(tokens):
            if tok == "-o" and i + 1 < len(tokens):
                tokens.pop(i)  # drop '-o'
                return tokens.pop(i)  # drop & return artifact
        return None


class ActionFilter(TokenFilter):
    def extract(self, tokens: list[str]) -> str | None:
        if "-c" in tokens:
            return CompileOrLink.COMPILE.value
        return CompileOrLink.LINK.value


def _parse(line: str) -> BuildArtifact | None:
    tokens = line.strip().split()
    if not tokens:
        return None

    # instantiate in the order we need them
    filters: list[TokenFilter] = [
        TimestampFilter(),
        InputArtifactFilter(),
        OutputArtifactFilter(),
        ActionFilter(),
    ]

    # apply each filter
    raw_ts = filters[0].extract(tokens)
    raw_in = filters[1].extract(tokens)
    raw_out = filters[2].extract(tokens)
    raw_act = filters[3].extract(tokens)

    if raw_ts is None or raw_in is None or raw_act is None:
        return None

    # the rest of `tokens` are the flags
    flags_str = " ".join(tokens)
    h = zlib.adler32(flags_str.encode("utf-8"))

    return BuildArtifact(
        timestamp=float(raw_ts),
        input_artifact=raw_in,
        output_artifact=raw_out,
        build_flags=flags_str,
        compile_or_link=CompileOrLink(raw_act),
        hash=h,
    )
