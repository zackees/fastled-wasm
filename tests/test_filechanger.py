"""
Unit test for file change notification.
"""

import os
import tempfile
import time
import unittest
from multiprocessing import Queue
from pathlib import Path

from fastled.filewatcher import FileChangedNotifier, create_file_watcher_process


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
            queue: Queue
            process, queue = create_file_watcher_process(Path(temp_dir), [])
            process.start()

            try:
                # Make a change to the file
                time.sleep(2)  # Give the watcher time to start
                test_file.write_text("new content")

                # Wait for and verify the change
                changed_file = queue.get(timeout=0.5)
                self.assertIsNotNone(changed_file, "No file change detected")
                self.assertEqual(
                    os.path.basename(changed_file), os.path.basename(str(test_file))
                )

            except Exception as e:
                type_str_of_exception = str(type(e))
                print(f"Got exception: {type_str_of_exception}")
                if "queue.Empty" in type_str_of_exception:
                    self.fail("No file change detected")
                print(f"Got exception: {e}")
                process.terminate()
                process.join()
                raise e

            finally:
                process.terminate()
                process.join()


if __name__ == "__main__":
    unittest.main()
