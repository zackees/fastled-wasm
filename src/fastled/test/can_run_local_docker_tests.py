import os
import platform


def can_run_local_docker_tests() -> bool:
    """Check if this system can run Docker Tests"""
    is_github_runner = "GITHUB_ACTIONS" in os.environ
    if not is_github_runner:
        from fastled.docker_manager import DockerManager

        return DockerManager.is_docker_installed()
    # this only works in ubuntu at the moment
    return platform.system() == "Linux"
