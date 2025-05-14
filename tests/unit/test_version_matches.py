"""
Unit test file.
"""

import shlex
import unittest
from pathlib import Path

HERE = Path(__file__).parent
PROJECT_ROOT = HERE.parent.parent

OUR_PY_PROJECT_TOML = PROJECT_ROOT / "pyproject.toml"
CONTAINER_PY_PROJECT_TOML = PROJECT_ROOT / "compiler" / "pyproject.toml"


def get_version_from_toml(toml_path: Path) -> str:
    """
    Extracts the fastled-wasm-server version from the pyproject.toml file.
    """
    needle = "fastled-wasm-server"
    version = ""
    text = toml_path.read_text()
    lines = text.splitlines()
    for line in lines:
        if needle in line:
            # use the regular expression to extract the version, be mindful of '"', and >= == < > and so on
            # match = re.search(rf"{needle}.*?=\s*\"(.*?)\"", line)
            # fails for "fastled-wasm-server>=1.0.5",  # This version can float upwards, while the docker is pinned.
            # remove comments
            line = line.split("#")[0]
            line = line.replace("'", '"')
            line = line.replace('"', "")
            line = line.replace(",", "")
            line = line.replace(">", " ")
            line = line.replace("<", " ")
            line = line.replace("=", " ")
            parts = shlex.split(line)
            if len(parts) < 2:
                return "any"
            version = parts[1]
            return version
    if not version:
        raise ValueError(f"Version not found in {toml_path}")
    return version


class VersionMatchesTester(unittest.TestCase):
    """Main tester class."""

    def test_sanity(self) -> None:
        self.assertTrue(
            OUR_PY_PROJECT_TOML.exists(),
            f"Expected {OUR_PY_PROJECT_TOML} to exist",
        )
        self.assertTrue(
            CONTAINER_PY_PROJECT_TOML.exists(),
            f"Expected {CONTAINER_PY_PROJECT_TOML} to exist",
        )

        our_version = get_version_from_toml(OUR_PY_PROJECT_TOML)
        container_version = get_version_from_toml(CONTAINER_PY_PROJECT_TOML)
        self.assertEqual(
            our_version,
            container_version,
            f"Versions do not match: {our_version} in {OUR_PY_PROJECT_TOML} != {container_version} in {CONTAINER_PY_PROJECT_TOML}",
        )


if __name__ == "__main__":
    unittest.main()
