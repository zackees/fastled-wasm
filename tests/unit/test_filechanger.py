"""
Unit test for file change notification.
"""

import os
import tempfile
import time
import unittest
from pathlib import Path

from fastled.filewatcher import FileWatcherProcess


class FileChangeProcessTester(unittest.TestCase):
    """Tests for process-based file watcher."""

    def test_process_file_change_detection(self) -> None:
        """Test that file changes are detected in a separate process."""
        # Create a temporary directory
        with tempfile.TemporaryDirectory() as temp_dir:
            # Create a test file
            test_file = Path(temp_dir) / "test.txt"
            test_file.write_text("initial content")

            # Start watching the directory in a separate process
            proc = FileWatcherProcess(Path(temp_dir), [])
            time.sleep(0.5)  # Give the watcher time to start

            try:
                # Make a change to the file
                # time.sleep(2)  # Give the watcher time to start
                test_file.write_text("new content")

                # Wait for and verify the change
                # changed_file = queue.get(timeout=5)
                changed_files = proc.get_all_changes(timeout=5)
                # self.assertIsNotNone(changed_file, "No file change detected")
                self.assertNotEqual(len(changed_files), 0, "No file change detected")
                self.assertEqual(
                    os.path.basename(changed_files[0]), os.path.basename(str(test_file))
                )
            except Exception as e:
                type_str_of_exception = str(type(e))
                print(f"Got exception: {type_str_of_exception}")
                if "queue.Empty" in type_str_of_exception:
                    self.fail("No file change detected")
                print(f"Got exception: {e}")
                raise e
            finally:
                proc.stop()


if __name__ == "__main__":
    unittest.main()
