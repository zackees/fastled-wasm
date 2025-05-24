import unittest

from fastled.util import banner_string

_MSG = "Hello, World!\nsecond line"

_EXPECTED: str = (
    "#################\n"
    "# Hello, World! #\n"
    "# second line   #\n"
    "#################"
)


class TestBannerString(unittest.TestCase):
    def test_banner_string(self):
        actual = banner_string(_MSG)
        print(actual)
        print(_EXPECTED)
        self.assertEqual(actual, _EXPECTED)


if __name__ == "__main__":
    unittest.main()
