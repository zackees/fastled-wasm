"""
Unit test file.
"""

import unittest
import warnings

from fastled import Api, Test  # type: ignore

_ALLOWED_FAILURE = 2  # Allow up to 2 failures


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
            for example, exc in out.items():
                if exc is not None:
                    print(f"Failed: {example} with {exc}")

            self.assertLessEqual(
                len(out),
                _ALLOWED_FAILURE,  #
                f"Failed tests: {out.keys()}",
            )
            if len(out) > _ALLOWED_FAILURE:
                warnings.warn(f"More than {_ALLOWED_FAILURE} failures: {out.keys()}")
            # self.assertEqual(0, len(out), f"Failed tests: {out.keys()}")


if __name__ == "__main__":
    unittest.main()
