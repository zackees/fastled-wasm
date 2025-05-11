"""
Unit test file.
"""

import unittest

from fastled.print_filter import BuildArtifact, PrintFilterFastled

_BUILD_CONFIG_SAMPLE = """
7.60 ccache em++ -o .pio/build/wasm/libbb9/fastled/fl/wave_simulation.o -c -std=c++17 -Werror=bad-function-cast -Werror=cast-function-type -I/js/src/platforms/wasm/compiler -g -O0 -gsource-map=inline -ffile-prefix-map=/=drawfsource/ -fsanitize=address -fsanitize=undefined -fno-inline -DPLATFORMIO=60117 -DFASTLED_NO_PINMAP -DHAS_HARDWARE_PIN_SUPPORT -DFASTLED_FORCE_SOFTWARE_SPI -Isrc -Isrc/platforms/wasm/compiler src/fl/wave_simulation.cpp
7.67 ccache em++ -o .pio/build/wasm/libbb9/fastled/fl/wave_simulation_real.o -c -std=c++17 -Werror=bad-function-cast -Werror=cast-function-type -I/js/src/platforms/wasm/compiler -g -O0 -gsource-map=inline -ffile-prefix-map=/=drawfsource/ -fsanitize=address -fsanitize=undefined -fno-inline -DPLATFORMIO=60117 -DFASTLED_NO_PINMAP -DHAS_HARDWARE_PIN_SUPPORT -DFASTLED_FORCE_SOFTWARE_SPI -Isrc -Isrc/platforms/wasm/compiler src/fl/wave_simulation_real.cpp
"""


# class ChunkedBuildConfigGrouper:
#     """
#     Groups compiler invocations by identical flag-sets.
#     Yields:
#       - A blank line + the flags when they change
#       - Then one line per file: "[time] filename"
#     """

#     _line_re = re.compile(
#         r'^\s*(?P<time>\d+\.\d+)\s+'
#         r'(?P<flags>.+?)\s+'
#         r'(?P<file>[^ ]+\.(?:cpp|c|ino))\s*$'
#     )

#     def __init__(self, echo: bool = True) -> None:
#         super().__init__(echo)
#         self._prev_key: str | None = None

#     def filter(self, text: str) -> list[str]:
#         out: list[str] = []
#         for raw in text.splitlines(keepends=True):
#             m = self._line_re.match(raw)
#             if not m:
#                 # passthrough anything that doesn’t match
#                 out.append(raw)
#                 continue

#             time_stamp = m.group('time')
#             flags     = m.group('flags')
#             src_file  = m.group('file')

#             # on a new flag‐set, emit a blank line + the flags
#             if flags != self._prev_key:
#                 self._prev_key = flags
#                 if out and not out[-1].endswith("\n\n"):
#                     out.append("\n")
#                 out.append(flags + "\n")

#             # then emit the timestamp + filename
#             out.append(f"{time_stamp} {src_file}\n")

#         return out


class PrintFitlerTester(unittest.TestCase):
    """Main tester class."""

    def test_print_filter(self) -> None:
        """Tests that a project can be filtered"""
        # Test the PrintFilter class
        pf = PrintFilterFastled(echo=False)
        pf.print("# WASM is building")  # This should trigger the filter.
        result = pf.print(
            "5.36 src/XYPath.ino.cpp:4:1: error: unknown type name 'kdsjfsdkfjsd'"
        )  # This should now be transformed.
        self.assertNotIn(".ino.cpp", result, "Expected .ino.cpp to be filtered out")
        self.assertIn(
            "examples/XYPath/XYPath.ino", result, "Expected path to be transformed"
        )

    # def test_print_filter_no_build(self) -> None:
    #     """Tests that a project can be filtered"""
    #     # Test the PrintFilter class
    #     cbc = ChunkedBuildConfigGrouper()

    #     print(f"\n###Filtering:\n\n{_BUILD_CONFIG_SAMPLE}")
    #     out = cbc.filter(_BUILD_CONFIG_SAMPLE)
    #     #print(out)
    #     print(f"###Filtered: result was {out}")
    #     print("Done")

    def test_build_artifact_parsing(self) -> None:
        """Tests that a project can be filtered"""
        # Test the PrintFilter class
        lines = _BUILD_CONFIG_SAMPLE.splitlines()
        lines = [line.strip() for line in lines if line.strip()]
        last_hash: int | None = None
        for line in lines:
            ba = BuildArtifact.parse(line)
            if ba is None:
                print("!!")
                continue

            if last_hash is None or ba.hash != last_hash:
                # print out the build flags
                print("Found new build flags:")
                print(ba.build_flags)
                # now set the hash to the new value
                last_hash = ba.hash

            # print the time and build output
            print(f"{ba.timestamp} {ba.output_artifact}")

        print("Done")


if __name__ == "__main__":
    unittest.main()
