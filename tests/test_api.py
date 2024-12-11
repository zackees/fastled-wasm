"""
Unit test file.
"""

import unittest

ENABLED = False


class ApiTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(ENABLED, "This test takes a long time.")
    def test_command(self) -> None:
        """Test command line interface (CLI)."""
        from fastled import Api, Test

        with Api.server() as server:
            out = Test.test_examples(host=server)
            self.assertEqual(0, len(out), f"Failed tests: {out}")


if __name__ == "__main__":
    unittest.main()
