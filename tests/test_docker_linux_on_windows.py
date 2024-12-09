import os
import unittest

from fastled.docker_manager import DockerManager

IS_GITHUB = "GITHUB_ACTIONS" in os.environ


class ProjectInitTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipIf(IS_GITHUB, "Skipping test on github")
    def test_compile(self) -> None:
        """Test web compilation functionality with real server."""
        # Test the web_compile function with actual server call
        ok = DockerManager.ensure_linux_containers_for_windows()
        self.assertTrue(ok)


if __name__ == "__main__":
    unittest.main()
