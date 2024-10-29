"""
Unit test for file change notification.
"""

import os
import tempfile
import time
import unittest
from pathlib import Path

from fastled_wasm.filewatcher import FileChangedNotifier


class FileChangeTester(unittest.TestCase):
    """Tests for FileChangedNotifier."""

    def test_file_change_detection(self) -> None:
        """Test that file changes are detected."""
        # Create a temporary directory
        with tempfile.TemporaryDirectory() as temp_dir:
            # Create a test file
            test_file = Path(temp_dir) / "test.txt"
            test_file.write_text("initial content")

            # Start watching the directory
            notifier = FileChangedNotifier(temp_dir)
            notifier.start()

            try:
                # Make a change to the file
                time.sleep(0.1)  # Give the watcher time to start
                test_file.write_text("new content")

                # Wait for and verify the change
                changed_file = notifier.get_next_change(timeout=0.5)
                self.assertIsNotNone(changed_file, "No file change detected")
                if changed_file:  # Type guard for mypy
                    self.assertEqual(
                        os.path.basename(changed_file), os.path.basename(str(test_file))
                    )

                # Verify no other changes pending
                self.assertIsNone(notifier.get_next_change(timeout=0.1))

            finally:
                notifier.stop()
                notifier.join()


if __name__ == "__main__":
    unittest.main()
