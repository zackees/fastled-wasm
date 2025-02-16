"""
Unit test file.
"""

import unittest
from pathlib import Path

from fastled import CompileServer, Docker, Test  # type: ignore

HERE = Path(__file__).parent
PROJECT_ROOT = HERE.parent
FASTLED_SISTER_REPO = PROJECT_ROOT / ".." / "FastLED"

DEFAULT_GITHUB_URL = "https://github.com/fastled/fastled"
OUTPUT_DIR = Path(".cache/fastled")


def _enabled() -> bool:
    """Check if this system can run the tests."""
    sister_repo_does_not_exist = not FASTLED_SISTER_REPO.exists()

    if sister_repo_does_not_exist:
        print(
            f"This test is only enable when FastLED is a repo in the same directly as the project root folder: {FASTLED_SISTER_REPO} does not exist"
        )
        return False

    return Test.can_run_local_docker_tests()


class BuildDockerFromGithubTester(unittest.TestCase):

    @unittest.skipUnless(_enabled(), "Skipping test on non-Linux system on github")
    def test_build_docker_from_github(self) -> None:
        """Builds the docker file from the fastled repo."""
        url = DEFAULT_GITHUB_URL
        print("Building from github")
        server: CompileServer = Docker.spawn_server_from_github(
            url=url, output_dir=OUTPUT_DIR
        )
        self.assertTrue(server, "Failed to build docker image")
        try:
            self.assertTrue(server.ping())
        finally:
            server.stop()


if __name__ == "__main__":
    unittest.main()
