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


class PrintFilterDefault(PrintFilter):
    """Provides default filtering for FastLED output."""

    def filter(self, text: str) -> str:
        return text
