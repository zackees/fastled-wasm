import unittest

from fastled.string_diff import is_in_order_match, string_diff

_HAYSTACK: list[str] = [
    "examples\\AnalogOutput",
    "examples\\Animartrix",
    "examples\\Apa102",
    "examples\\Apa102HD",
    "examples\\Apa102HDOverride",
    "examples\\Blink",
    "examples\\BlinkParallel",
    "examples\\Blur",
    "examples\\Blur2d",
    "examples\\Chromancer",
    "examples\\ColorPalette",
    "examples\\ColorTemperature",
    "examples\\Cylon",
    "examples\\DemoReel100",
    "examples\\Esp32S3I2SDemo",
    "examples\\EspI2SDemo",
    "examples\\Fire2012",
    "examples\\Fire2012WithPalette",
    "examples\\Fire2023",
    "examples\\FireCylinder",
    "examples\\FireMatrix",
    "examples\\FirstLight",
    "examples\\FxCylon",
    "examples\\FxDemoReel100",
    "examples\\FxEngine",
    "examples\\FxFire2012",
    "examples\\FxGfx2Video",
    "examples\\FxNoisePlusPalette",
    "examples\\FxNoiseRing",
    "examples\\FxPacifica",
    "examples\\FxPride2015",
    "examples\\FxSdCard",
    "examples\\FxTwinkleFox",
    "examples\\FxWater",
    "examples\\FxWave2d",
    "examples\\HD107",
    "examples\\LuminescentGrand",
    "examples\\Noise",
    "examples\\NoisePlayground",
    "examples\\NoisePlusPalette",
    "examples\\OctoWS2811",
    "examples\\Overclock",
    "examples\\Pacifica",
    "examples\\PinMode",
    "examples\\Pintest",
    "examples\\Pride2015",
    "examples\\RGBCalibrate",
    "examples\\RGBSetDemo",
    "examples\\RGBW",
    "examples\\RGBWEmulated",
    "examples\\SmartMatrix",
    "examples\\TeensyMassiveParallel",
    "examples\\TeensyParallel",
    "examples\\TwinkleFox",
    "examples\\wasm",
    "examples\\WasmScreenCoords",
    "examples\\Wave",
    "examples\\Wave2d",
    "examples\\WS2816",
]


class StringDiffTester(unittest.TestCase):
    """Main tester class."""

    def test_needle_in_hastack(self) -> None:
        """Test if the needle is in the haystack."""
        result = string_diff("FxWave", _HAYSTACK)
        self.assertGreater(len(result), 0)
        _, path = result[0]
        self.assertEqual("examples\\FxWave2d", path)

    def test_is_in_order_match(self) -> None:
        is_match = is_in_order_match("wave 2d", "wave2d")
        self.assertTrue(is_match)

    def test_wave_2d(self) -> None:
        needle = "wave 2d"
        result = string_diff(needle, _HAYSTACK)
        self.assertGreater(len(result), 0)
        _, path = result[0]
        self.assertEqual("examples\\Wave2d", path)


if __name__ == "__main__":
    unittest.main()
