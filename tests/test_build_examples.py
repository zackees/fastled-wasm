"""
Unit test file.
"""

import unittest

from fastled import Api, Test  # type: ignore


def _enabled() -> bool:
    """Check if this system can run the tests."""
    from fastled import Test

    return Test.can_run_local_docker_tests()


class ApiTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(_enabled(), "Skipping test on non-Linux system on github")
    def test_build_all_examples(self) -> None:
        """Test command line interface (CLI)."""

        with Api.server(auto_updates=True) as server:
            out = Test.test_examples(host=server)
            self.assertEqual(0, len(out), f"Failed tests: {out}")


if __name__ == "__main__":
    unittest.main()
