#!/usr/bin/env python3
"""
Comprehensive test for string_diff function using FastLED examples.
Tests various substring patterns, spacing, and edge cases.
"""

import sys
from pathlib import Path

# Add the src directory to the path so we can import string_diff
sys.path.insert(0, str(Path(__file__).parent.parent.parent / "src"))

from fastled.string_diff import string_diff, string_diff_paths


class TestStringDiffComprehensive:
    """Comprehensive test suite for string_diff function."""

    @classmethod
    def setup_class(cls):
        """Set up test data - create a list of FastLED example names."""
        cls.fastled_examples = [
            "AnalogOutput",
            "Animartrix",
            "Apa102",
            "Apa102HD",
            "Apa102HDOverride",
            "Audio",
            "Blink",
            "BlinkParallel",
            "Blur",
            "Blur2d",
            "Chromancer",
            "ColorBoost",
            "ColorPalette",
            "ColorTemperature",
            "CompileTest",
            "Corkscrew",
            "Cylon",
            "DemoReel100",
            "Downscale",
            "EaseInOut",
            "Esp32S3I2SDemo",
            "EspI2SDemo",
            "FestivalStick",
            "Fire2012",
            "Fire2012WithPalette",
            "Fire2023",
            "FireCylinder",
            "FireMatrix",
            "FirstLight",
            "FunkyClouds",
            "FxCylon",
            "FxDemoReel100",
            "FxEngine",
            "FxFire2012",
            "FxGfx2Video",
            "FxNoisePlusPalette",
            "FxNoiseRing",
            "FxPacifica",
            "FxPride2015",
            "FxSdCard",
            "FxTwinkleFox",
            "FxWater",
            "FxWave2d",
            "HD107",
            "HSVTest",
            "JsonConsole",
            "LuminescentGrand",
            "Multiple",
            "Noise",
            "NoisePlayground",
            "NoisePlusPalette",
            "OctoWS2811",
            "Overclock",
            "Pacifica",
            "PinMode",
            "Pintest",
            "Ports",
            "Pride2015",
            "RGBCalibrate",
            "RGBSetDemo",
            "RGBW",
            "RGBWEmulated",
            "SmartMatrix",
            "TeensyMassiveParallel",
            "TeensyParallel",
            "TwinkleFox",
            "UITest",
            "wasm",
            "WasmScreenCoords",
            "Wave",
            "Wave2d",
            "WS2816",
            "XYMatrix",
            "XYPath",
        ]

        # Create example paths like "examples/Blink" format
        cls.example_paths = [f"examples/{name}" for name in cls.fastled_examples]

    def test_exact_matches(self):
        """Test that exact matches return the exact match (and variants if they exist)."""
        test_cases = [
            ("Pacifica", 1),  # Unique match
            ("Fire2012", 3),  # Has variants: Fire2012, Fire2012WithPalette, FxFire2012
            ("Cylon", 1),  # Unique match
            ("Pride2015", 2),  # Has variant: Pride2015, FxPride2015
            ("JsonConsole", 1),  # Unique match
        ]

        for test_case, expected_count in test_cases:
            results = string_diff(test_case, self.fastled_examples)
            assert (
                len(results) >= expected_count
            ), f"Expected at least {expected_count} match(es) for '{test_case}', got {len(results)}: {results}"
            # Ensure the exact match is in the results
            result_names = [r[1] for r in results]
            assert (
                test_case in result_names
            ), f"Expected exact match '{test_case}' in results, got: {result_names}"

    def test_case_insensitive_matches(self):
        """Test case insensitive matching."""
        test_cases = [
            ("pacifica", "Pacifica"),  # Unique
            ("FIRE2012", "Fire2012"),  # Has variants
            ("cylon", "Cylon"),  # Unique
            ("pride2015", "Pride2015"),  # Has variants
            ("jsonConsole", "JsonConsole"),  # Unique
        ]

        for input_str, expected in test_cases:
            results = string_diff(input_str, self.fastled_examples)
            result_names = [r[1] for r in results]
            assert (
                expected in result_names
            ), f"Expected '{expected}' in results for '{input_str}', got: {result_names}"

    def test_partial_matches(self):
        """Test partial substring matching."""
        test_cases = [
            (
                "Fire",
                [
                    "Fire2012",
                    "Fire2012WithPalette",
                    "Fire2023",
                    "FireCylinder",
                    "FireMatrix",
                    "FxFire2012",
                ],
            ),
            (
                "Fx",
                [
                    "FxCylon",
                    "FxDemoReel100",
                    "FxEngine",
                    "FxFire2012",
                    "FxGfx2Video",
                    "FxNoisePlusPalette",
                    "FxNoiseRing",
                    "FxPacifica",
                    "FxPride2015",
                    "FxSdCard",
                    "FxTwinkleFox",
                    "FxWater",
                    "FxWave2d",
                ],
            ),
            ("Blur", ["Blur", "Blur2d"]),
            (
                "Noise",
                [
                    "Noise",
                    "NoisePlayground",
                    "NoisePlusPalette",
                    "FxNoisePlusPalette",
                    "FxNoiseRing",
                ],
            ),
            ("RGB", ["RGBCalibrate", "RGBSetDemo", "RGBW", "RGBWEmulated"]),
        ]

        for input_str, expected_matches in test_cases:
            results = string_diff(input_str, self.fastled_examples)
            result_names = [r[1] for r in results]

            # Check that all expected matches are found
            for expected in expected_matches:
                assert (
                    expected in result_names
                ), f"Expected '{expected}' in results for '{input_str}', got: {result_names}"

    def test_spaces_in_input(self):
        """Test that spaces in input are handled correctly."""
        test_cases = [
            ("B l i n k", "Blink"),
            ("F i r e 2 0 1 2", "Fire2012"),
            ("D e m o R e e l 1 0 0", "DemoReel100"),
            ("P a c i f i c a", "Pacifica"),
            ("P r i d e 2 0 1 5", "Pride2015"),
        ]

        for input_str, expected in test_cases:
            results = string_diff(input_str, self.fastled_examples)
            result_names = [r[1] for r in results]
            assert (
                expected in result_names
            ), f"Expected '{expected}' in results for spaced input '{input_str}', got: {result_names}"

    def test_typos_and_minor_edits(self):
        """Test handling of typos and minor edits."""
        test_cases = [
            ("Blik", "Blink"),
            ("Fire201", "Fire2012"),
            ("DemoRel100", "DemoReel100"),
            ("Pacifca", "Pacifica"),
            ("Prid2015", "Pride2015"),
            ("Cylom", "Cylon"),
            ("Matric", "XYMatrix"),
            ("Nois", "Noise"),
        ]

        for input_str, expected in test_cases:
            results = string_diff(input_str, self.fastled_examples)
            result_names = [r[1] for r in results]
            assert (
                expected in result_names
            ), f"Expected '{expected}' in results for typo '{input_str}', got: {result_names}"

    def test_prefix_matching(self):
        """Test prefix matching behavior."""
        test_cases = [
            ("Bl", ["Blink", "BlinkParallel", "Blur", "Blur2d"]),
            (
                "Fi",
                [
                    "Fire2012",
                    "Fire2012WithPalette",
                    "Fire2023",
                    "FireCylinder",
                    "FireMatrix",
                    "FirstLight",
                ],
            ),
            (
                "Fx",
                [
                    "FxCylon",
                    "FxDemoReel100",
                    "FxEngine",
                    "FxFire2012",
                    "FxGfx2Video",
                    "FxNoisePlusPalette",
                    "FxNoiseRing",
                    "FxPacifica",
                    "FxPride2015",
                    "FxSdCard",
                    "FxTwinkleFox",
                    "FxWater",
                    "FxWave2d",
                ],
            ),
            ("RGB", ["RGBCalibrate", "RGBSetDemo", "RGBW", "RGBWEmulated"]),
        ]

        for input_str, expected_matches in test_cases:
            results = string_diff(input_str, self.fastled_examples)
            result_names = [r[1] for r in results]

            # Check that all expected matches are found
            for expected in expected_matches:
                assert (
                    expected in result_names
                ), f"Expected '{expected}' in results for prefix '{input_str}', got: {result_names}"

    def test_suffix_matching(self):
        """Test suffix matching behavior."""
        test_cases = [
            ("2012", ["Fire2012", "Fire2012WithPalette", "FxFire2012"]),
            ("2015", ["Pride2015", "FxPride2015"]),
            ("Matrix", ["FireMatrix", "SmartMatrix", "XYMatrix"]),
            ("Test", ["CompileTest", "HSVTest", "UITest"]),
        ]

        for input_str, expected_matches in test_cases:
            results = string_diff(input_str, self.fastled_examples)
            result_names = [r[1] for r in results]

            # Check that all expected matches are found
            for expected in expected_matches:
                assert (
                    expected in result_names
                ), f"Expected '{expected}' in results for suffix '{input_str}', got: {result_names}"

    def test_single_character_inputs(self):
        """Test single character inputs."""
        test_cases = [
            ("B", ["Blink", "BlinkParallel", "Blur", "Blur2d", "ColorBoost"]),
            (
                "F",
                [
                    "FestivalStick",
                    "Fire2012",
                    "Fire2012WithPalette",
                    "Fire2023",
                    "FireCylinder",
                    "FireMatrix",
                    "FirstLight",
                    "FunkyClouds",
                    "FxCylon",
                    "FxDemoReel100",
                    "FxEngine",
                    "FxFire2012",
                    "FxGfx2Video",
                    "FxNoisePlusPalette",
                    "FxNoiseRing",
                    "FxPacifica",
                    "FxPride2015",
                    "FxSdCard",
                    "FxTwinkleFox",
                    "FxWater",
                    "FxWave2d",
                ],
            ),
            (
                "X",
                [
                    "FxCylon",
                    "FxDemoReel100",
                    "FxEngine",
                    "FxFire2012",
                    "FxGfx2Video",
                    "FxNoisePlusPalette",
                    "FxNoiseRing",
                    "FxPacifica",
                    "FxPride2015",
                    "FxSdCard",
                    "FxTwinkleFox",
                    "FxWater",
                    "FxWave2d",
                    "XYMatrix",
                    "XYPath",
                ],
            ),
        ]

        for input_str, expected_matches in test_cases:
            results = string_diff(input_str, self.fastled_examples)
            result_names = [r[1] for r in results]

            # Check that all expected matches are found
            for expected in expected_matches:
                assert (
                    expected in result_names
                ), f"Expected '{expected}' in results for single char '{input_str}', got: {result_names}"

    def test_path_matching(self):
        """Test path-based matching using string_diff_paths."""
        path_objects = [Path(p) for p in self.example_paths]

        test_cases = [
            ("examples/Blink", "examples/Blink"),
            ("Blink", "examples/Blink"),
            ("examples/Fire2012", "examples/Fire2012"),
            ("Fire2012", "examples/Fire2012"),
        ]

        for input_str, expected in test_cases:
            results = string_diff_paths(input_str, path_objects)
            result_paths = [str(r[1]) for r in results]

            # Check for both forward and backward slashes (platform-agnostic)
            expected_forward = expected
            expected_backward = expected.replace("/", "\\")

            path_found = any(
                expected_forward in result_paths
                or expected_backward in result_paths
                or str(r[1]).replace("\\", "/") == expected_forward
                for r in results
            )

            assert (
                path_found
            ), f"Expected '{expected}' (or equivalent) in path results for '{input_str}', got: {result_paths}"

    def test_unique_results_for_specific_inputs(self):
        """Test that specific inputs return unique results."""
        # These should return exactly one result (no similar variants exist)
        unique_test_cases = [
            "AnalogOutput",
            "Animartrix",
            "Audio",
            "Chromancer",
            "ColorBoost",
            "FirstLight",
            "JsonConsole",
            "Multiple",
            "Overclock",
            "UITest",
        ]

        for test_case in unique_test_cases:
            results = string_diff(test_case, self.fastled_examples)
            assert (
                len(results) == 1
            ), f"Expected exactly 1 unique result for '{test_case}', got {len(results)}: {[r[1] for r in results]}"

    def test_ambiguous_inputs(self):
        """Test inputs that should return multiple results."""
        ambiguous_test_cases = [
            (
                "Fire",
                6,
            ),  # Should match Fire2012, Fire2012WithPalette, Fire2023, FireCylinder, FireMatrix, FxFire2012
            ("Blur", 2),  # Should match Blur, Blur2d
            ("Wave", 3),  # Should match Wave, Wave2d, FxWave2d
            ("RGB", 4),  # Should match RGBCalibrate, RGBSetDemo, RGBW, RGBWEmulated
        ]

        for input_str, expected_count in ambiguous_test_cases:
            results = string_diff(input_str, self.fastled_examples)
            assert (
                len(results) >= expected_count
            ), f"Expected at least {expected_count} results for ambiguous input '{input_str}', got {len(results)}: {[r[1] for r in results]}"

    def test_empty_and_invalid_inputs(self):
        """Test edge cases with empty and invalid inputs."""
        edge_cases = ["", " ", "   ", "xyz123notfound", "!@#$%^&*()", "1234567890"]

        for input_str in edge_cases:
            results = string_diff(input_str, self.fastled_examples)
            # Should return some results (fuzzy matching should find something)
            assert (
                len(results) > 0
            ), f"Expected some results for edge case '{input_str}', got {len(results)}"

    def test_very_long_inputs(self):
        """Test very long inputs."""
        long_inputs = [
            "BlinkBlinkBlinkBlinkBlinkBlinkBlink",
            "Fire2012Fire2012Fire2012Fire2012Fire2012",
            "a" * 100,
            "ThisIsAVeryLongInputThatShouldStillWork",
        ]

        for input_str in long_inputs:
            results = string_diff(input_str, self.fastled_examples)
            # Should return some results
            assert (
                len(results) > 0
            ), f"Expected some results for long input '{input_str[:20]}...', got {len(results)}"

    def test_special_characters_in_input(self):
        """Test inputs with special characters."""
        special_cases = [
            ("Blink_", "Blink"),
            ("Fire-2012", "Fire2012"),
            ("Demo.Reel.100", "DemoReel100"),
            ("Pacifica!", "Pacifica"),
            ("Pride@2015", "Pride2015"),
        ]

        for input_str, expected in special_cases:
            results = string_diff(input_str, self.fastled_examples)
            result_names = [r[1] for r in results]
            assert (
                expected in result_names
            ), f"Expected '{expected}' in results for special char input '{input_str}', got: {result_names}"

    def test_performance_with_large_dataset(self):
        """Test performance with a larger dataset."""
        # Create a larger dataset by duplicating examples with variations
        large_dataset = self.fastled_examples.copy()
        for example in self.fastled_examples:
            large_dataset.extend(
                [
                    f"{example}_v1",
                    f"{example}_v2",
                    f"{example}_old",
                    f"{example}_new",
                    f"modified_{example}",
                ]
            )

        # Test that it still works efficiently
        test_cases = ["Blink", "Fire2012", "DemoReel100", "Pacifica"]

        for test_case in test_cases:
            results = string_diff(test_case, large_dataset)
            # Should still find the original example
            result_names = [r[1] for r in results]
            assert (
                test_case in result_names
            ), f"Expected '{test_case}' in results from large dataset, got: {result_names}"

    def test_consistency_across_runs(self):
        """Test that results are consistent across multiple runs."""
        test_cases = ["Blink", "Fire2012", "DemoReel100"]

        for test_case in test_cases:
            results1 = string_diff(test_case, self.fastled_examples)
            results2 = string_diff(test_case, self.fastled_examples)
            results3 = string_diff(test_case, self.fastled_examples)

            # Results should be identical
            assert (
                results1 == results2 == results3
            ), f"Results inconsistent for '{test_case}'"

    def test_investigate_multiple_results(self):
        """Investigate cases where multiple results are returned and understand why."""
        # Test cases that might return multiple results
        investigation_cases = [
            "Bl",  # Should match Blink, BlinkParallel, Blur, Blur2d
            "Fire",  # Should match multiple Fire* examples
            "Fx",  # Should match all Fx* examples
            "RGB",  # Should match RGB* examples
            "2012",  # Should match *2012* examples
            "Matrix",  # Should match *Matrix examples
        ]

        for input_str in investigation_cases:
            results = string_diff(input_str, self.fastled_examples)
            result_names = [r[1] for r in results]

            print(f"\nInput: '{input_str}' -> {len(results)} results:")
            for i, name in enumerate(result_names):
                print(f"  {i+1}. {name}")

            # For debugging - check if results make sense
            if len(results) > 1:
                # All results should contain the input string or be similar
                for result_name in result_names:
                    contains_input = input_str.lower() in result_name.lower()
                    print(
                        f"    '{result_name}' contains '{input_str}': {contains_input}"
                    )


if __name__ == "__main__":
    # Run the tests
    test_instance = TestStringDiffComprehensive()
    test_instance.setup_class()

    # Run individual test methods
    test_methods = [
        test_instance.test_exact_matches,
        test_instance.test_case_insensitive_matches,
        test_instance.test_partial_matches,
        test_instance.test_spaces_in_input,
        test_instance.test_typos_and_minor_edits,
        test_instance.test_prefix_matching,
        test_instance.test_suffix_matching,
        test_instance.test_single_character_inputs,
        test_instance.test_path_matching,
        test_instance.test_unique_results_for_specific_inputs,
        test_instance.test_ambiguous_inputs,
        test_instance.test_empty_and_invalid_inputs,
        test_instance.test_very_long_inputs,
        test_instance.test_special_characters_in_input,
        test_instance.test_performance_with_large_dataset,
        test_instance.test_consistency_across_runs,
        test_instance.test_investigate_multiple_results,
    ]

    print("Running comprehensive string_diff tests...")
    for i, test_method in enumerate(test_methods):
        try:
            test_method()
            print(f"✓ {test_method.__name__}")
        except Exception as e:
            print(f"✗ {test_method.__name__}: {e}")
            import traceback

            traceback.print_exc()

    print("\nAll tests completed!")
