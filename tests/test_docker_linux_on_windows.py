import unittest

from fastled.docker_manager import DockerManager


class ProjectInitTester(unittest.TestCase):
    """Main tester class."""

    def test_compile(self) -> None:
        """Test web compilation functionality with real server."""
        # Test the web_compile function with actual server call
        ok = DockerManager.ensure_linux_containers_for_windows()
        self.assertTrue(ok)


if __name__ == "__main__":
    unittest.main()
