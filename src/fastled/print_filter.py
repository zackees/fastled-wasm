import re


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


class PrintFilter:
    """Provides filtering for text output so that source files match up with local names."""

    def __init__(self, echo: bool = True) -> None:
        self.echo = echo
        self.build_started = False
        pass

    def _filter_all(self, text: str) -> str:
        lines = text.splitlines()
        out: list[str] = []
        for line in lines:
            if "# WASM is building" in line:
                self.build_started = True
            if self.build_started:
                line = _handle_ino_cpp(line)
            out.append(line)
        text = "\n".join(out)
        return text

    def print(self, text: str | bytes) -> str:
        """Prints the text to the console."""
        if isinstance(text, bytes):
            text = text.decode("utf-8")
        text = self._filter_all(text)
        if self.echo:
            print(text, end="")
        return text
