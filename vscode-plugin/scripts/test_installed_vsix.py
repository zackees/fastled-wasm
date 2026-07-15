#!/usr/bin/env python3
"""Run the Extension Development Host suite against an extracted, clean VSIX."""
import argparse
import os
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument('--vsix', type=Path, required=True)
    parser.add_argument('--expected', choices=('native', 'universal'), required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        with zipfile.ZipFile(args.vsix) as archive:
            archive.extractall(root)
        extension = root / 'extension'
        if not (extension / 'package.json').is_file():
            raise ValueError('VSIX does not contain an extension root')
        env = os.environ.copy()
        env['FASTLED_TEST_EXTENSION_PATH'] = str(extension)
        env['FASTLED_EXPECT_BUNDLED_CLANGD'] = args.expected
        # The extension process gets this empty PATH through runTests; retaining
        # the launcher PATH here is necessary to start Node and VS Code.
        command = ['node', 'out/test/runTest.js']
        if os.name != 'nt' and os.uname().sysname == 'Linux':
            # Hosted Linux runners have no desktop session.  VS Code's
            # Extension Development Host still needs an X server even though
            # the test itself is non-interactive.
            command = ['xvfb-run', '--auto-servernum', *command]
        subprocess.run(command, cwd=ROOT, env=env, check=True)


if __name__ == '__main__':
    main()
