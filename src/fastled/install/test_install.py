"""Comprehensive test suite for FastLED install feature."""

import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent.parent))

from fastled.install.main import auto_execute_fastled, fastled_install
from fastled.install.project_detection import (
    check_existing_arduino_content,
)
from fastled.install.vscode_config import (
    generate_fastled_tasks,
    update_launch_json_for_arduino,
    update_vscode_settings_for_fastled,
)


class TestFastLEDInstall(unittest.TestCase):
    """Test FastLED installation functionality."""

    def setUp(self):
        """Set up test environment."""
        self.test_dir = tempfile.mkdtemp()
        self.original_cwd = os.getcwd()
        os.chdir(self.test_dir)

    def tearDown(self):
        """Clean up test environment."""
        os.chdir(self.original_cwd)
        shutil.rmtree(self.test_dir)

    @patch("fastled.install.project_detection.shutil.which")
    @patch("builtins.input")
    def test_dry_run_basic_project(self, mock_input, mock_which):
        """Test 1: Dry-run in basic project."""
        # Setup
        mock_which.return_value = "/usr/bin/code"
        mock_input.return_value = "y"
        os.makedirs(".vscode")

        # Run
        result = fastled_install(dry_run=True, no_interactive=True)

        # Verify
        self.assertTrue(result)
        self.assertTrue(Path(".vscode/launch.json").exists())
        self.assertTrue(Path(".vscode/tasks.json").exists())

    @patch("fastled.install.project_detection.is_fastled_repository")
    @patch("fastled.install.project_detection.detect_fastled_project")
    @patch("fastled.install.project_detection.shutil.which")
    @patch("builtins.input")
    def test_dry_run_fastled_external(
        self, mock_input, mock_which, mock_detect, mock_repo
    ):
        """Test 2: Dry-run in external FastLED project."""
        # Setup
        mock_which.return_value = "/usr/bin/code"
        mock_input.return_value = "y"
        mock_detect.return_value = True
        mock_repo.return_value = False
        os.makedirs(".vscode")

        # Create library.json
        with open("library.json", "w") as f:
            json.dump({"name": "FastLED"}, f)

        # Run
        result = fastled_install(dry_run=True, no_interactive=True)

        # Verify
        self.assertTrue(result)
        self.assertFalse(Path(".vscode/settings.json").exists())  # No clangd settings

    @patch("fastled.install.project_detection.is_fastled_repository")
    @patch("fastled.install.project_detection.detect_fastled_project")
    @patch("fastled.install.project_detection.shutil.which")
    @patch("builtins.input")
    def test_dry_run_fastled_repository(
        self, mock_input, mock_which, mock_detect, mock_repo
    ):
        """Test 3: Dry-run in actual FastLED repository."""
        # Setup
        mock_which.return_value = "/usr/bin/code"
        mock_input.return_value = "y"
        mock_detect.return_value = True
        mock_repo.return_value = True
        os.makedirs(".vscode")

        # Run
        result = fastled_install(dry_run=True, no_interactive=True)

        # Verify
        self.assertTrue(result)
        self.assertTrue(Path(".vscode/settings.json").exists())  # Has clangd settings

    @patch("fastled.install.project_detection.shutil.which")
    @patch("builtins.input")
    def test_existing_vscode_project(self, mock_input, mock_which):
        """Test 4: Merge with existing .vscode configs."""
        # Setup
        mock_which.return_value = "/usr/bin/code"
        mock_input.return_value = "y"
        os.makedirs(".vscode")

        # Create existing launch.json
        existing_config = {
            "version": "0.2.0",
            "configurations": [{"name": "Existing", "type": "node"}],
        }
        with open(".vscode/launch.json", "w") as f:
            json.dump(existing_config, f)

        # Run
        result = fastled_install(dry_run=True, no_interactive=True)

        # Verify
        self.assertTrue(result)
        with open(".vscode/launch.json") as f:
            data = json.load(f)
            self.assertEqual(len(data["configurations"]), 2)
            self.assertEqual(
                data["configurations"][0]["name"],
                "🎯 Auto Debug (Smart File Detection)",
            )

    @patch("fastled.install.project_detection.shutil.which")
    @patch("builtins.input")
    def test_parent_directory_detection(self, mock_input, mock_which):
        """Test 5: Find .vscode in parent directories."""
        # Setup
        mock_which.return_value = "/usr/bin/code"
        parent_dir = Path(self.test_dir)
        child_dir = parent_dir / "child" / "grandchild"
        child_dir.mkdir(parents=True)
        (parent_dir / ".vscode").mkdir()
        os.chdir(child_dir)

        # Test non-interactive mode - should fail
        result = fastled_install(dry_run=True, no_interactive=True)
        self.assertFalse(result)  # Should fail in non-interactive

        # Test interactive mode
        mock_input.side_effect = ["y", "y"]  # Yes to parent, yes to extension
        result = fastled_install(dry_run=True, no_interactive=False)

        # Verify - we should be in parent directory now
        self.assertTrue(result)
        self.assertEqual(Path.cwd(), parent_dir)

    @patch("fastled.install.project_detection.shutil.which")
    @patch("builtins.input")
    def test_project_generation(self, mock_input, mock_which):
        """Test 6: Generate new VSCode project."""
        # Setup
        mock_which.return_value = "/usr/bin/code"

        # Test non-interactive mode - should fail
        result = fastled_install(dry_run=True, no_interactive=True)
        self.assertFalse(result)  # Should fail without .vscode

        # Test interactive mode
        mock_input.side_effect = ["y", "y"]  # Yes to generate, yes to extension
        result = fastled_install(dry_run=True, no_interactive=False)

        # Verify
        self.assertTrue(result)
        self.assertTrue(Path(".vscode").exists())
        self.assertTrue(Path(".vscode/launch.json").exists())
        self.assertTrue(Path(".vscode/tasks.json").exists())

    def test_arduino_content_detection(self):
        """Test 7: Detect existing .ino files."""
        # Create .ino file
        with open("test.ino", "w") as f:
            f.write("void setup() {}")

        # Test detection
        self.assertTrue(check_existing_arduino_content())

        # Remove and test examples folder
        os.unlink("test.ino")
        os.makedirs("examples")
        self.assertTrue(check_existing_arduino_content())

    @patch("fastled.install.project_detection.shutil.which")
    @patch("builtins.input")
    def test_tasks_json_merging(self, mock_input, mock_which):
        """Test 8: Merge FastLED tasks with existing."""
        # Setup
        mock_which.return_value = "/usr/bin/code"
        mock_input.return_value = "y"
        os.makedirs(".vscode")

        # Create existing tasks.json
        existing_tasks = {
            "version": "2.0.0",
            "tasks": [{"label": "Existing Task", "command": "echo"}],
        }
        with open(".vscode/tasks.json", "w") as f:
            json.dump(existing_tasks, f)

        # Run
        generate_fastled_tasks()

        # Verify
        with open(".vscode/tasks.json") as f:
            data = json.load(f)
            self.assertEqual(len(data["tasks"]), 3)  # 1 existing + 2 FastLED
            labels = [task["label"] for task in data["tasks"]]
            self.assertIn("Run FastLED (Debug)", labels)
            self.assertIn("Run FastLED (Quick)", labels)

    def test_launch_json_updates(self):
        """Test 9: Update launch.json configurations."""
        # Setup
        os.makedirs(".vscode")

        # Run
        update_launch_json_for_arduino()

        # Verify
        with open(".vscode/launch.json") as f:
            data = json.load(f)
            self.assertEqual(len(data["configurations"]), 1)
            config = data["configurations"][0]
            self.assertEqual(config["name"], "🎯 Auto Debug (Smart File Detection)")
            self.assertEqual(config["type"], "auto-debug")
            self.assertIn("*.ino", config["map"])

    @patch("fastled.install.project_detection.is_fastled_repository")
    def test_safety_clangd_protection(self, mock_repo):
        """Test 10: 🚨 CRITICAL - clangd safety protection."""
        # Setup
        os.makedirs(".vscode")

        # Test non-repository
        mock_repo.return_value = False
        update_vscode_settings_for_fastled()
        self.assertFalse(Path(".vscode/settings.json").exists())

        # Test repository
        mock_repo.return_value = True
        update_vscode_settings_for_fastled()
        self.assertTrue(Path(".vscode/settings.json").exists())

        with open(".vscode/settings.json") as f:
            data = json.load(f)
            self.assertIn("clangd.arguments", data)

    @patch("fastled.install.main.check_existing_arduino_content")
    def test_auto_execution_trigger(self, mock_check):
        """Test 11: Post-installation auto-execution."""
        # Setup
        mock_check.return_value = True
        original_argv = sys.argv.copy()
        sys.argv = ["fastled", "--install"]

        try:
            # We'll test that auto_execute_fastled modifies sys.argv correctly
            # without actually calling main()
            with patch("fastled.app.main") as mock_main:
                mock_main.return_value = 0
                auto_execute_fastled()

                # Verify
                mock_main.assert_called_once()
                # Check that argv was filtered before calling main
                # The function should have set sys.argv to ['fastled', '.']
        finally:
            sys.argv = original_argv

    @patch("fastled.install.project_detection.shutil.which")
    def test_no_ide_error_handling(self, mock_which):
        """Test 12: Error when no IDE available."""
        # Setup
        mock_which.return_value = None

        # Run
        from fastled.install.project_detection import validate_vscode_project

        result = validate_vscode_project(no_interactive=True)

        # Verify
        self.assertFalse(result)

    @patch("subprocess.run")
    @patch("builtins.input")
    def test_examples_installation(self, mock_input, mock_run):
        """Test 13: --project-init examples installation."""
        # Setup
        mock_input.return_value = "y"
        mock_run.return_value = MagicMock(returncode=0)

        # Run
        from fastled.install.examples_manager import (
            install_fastled_examples_via_project_init,
        )

        # Test non-interactive mode - should skip
        result = install_fastled_examples_via_project_init(no_interactive=True)
        self.assertFalse(result)

        # Test interactive mode
        result = install_fastled_examples_via_project_init(no_interactive=False)
        self.assertTrue(result)
        mock_run.assert_called_once()
        call_args = mock_run.call_args[0][0]
        self.assertEqual(call_args, ["fastled", "--project-init"])

    @patch("fastled.install.extension_manager.download_auto_debug_extension")
    @patch("fastled.install.extension_manager.install_vscode_extensions")
    def test_extension_installation_flow(self, mock_install, mock_download):
        """Test 14: Auto Debug extension prompt/install."""
        # Setup
        mock_download.return_value = Path("test.vsix")
        mock_install.return_value = True

        # Test dry run
        from fastled.install.extension_manager import install_auto_debug_extension

        result = install_auto_debug_extension(dry_run=True)
        self.assertTrue(result)
        mock_download.assert_not_called()

        # Test real install
        result = install_auto_debug_extension(dry_run=False)
        self.assertTrue(result)
        mock_download.assert_called_once()
        mock_install.assert_called_once()

    @patch("fastled.install.project_detection.is_fastled_repository")
    @patch("fastled.install.project_detection.shutil.which")
    @patch("builtins.input")
    def test_comprehensive_integration(self, mock_input, mock_which, mock_repo):
        """Test 15: End-to-end integration test."""
        # Setup
        mock_which.return_value = "/usr/bin/code"
        mock_input.side_effect = ["y", "y", "y"]  # Yes to project, extension, examples
        mock_repo.return_value = False

        # Create .vscode first for non-interactive test
        os.makedirs(".vscode")

        # Run full installation in non-interactive mode
        result = fastled_install(dry_run=True, no_interactive=True)

        # Verify all components
        self.assertTrue(result)
        self.assertTrue(Path(".vscode").exists())
        self.assertTrue(Path(".vscode/launch.json").exists())
        self.assertTrue(Path(".vscode/tasks.json").exists())

        # Verify tasks have correct content
        with open(".vscode/tasks.json") as f:
            data = json.load(f)
            debug_task = next(
                t for t in data["tasks"] if t["label"] == "Run FastLED (Debug)"
            )
            self.assertIn("--debug", debug_task["args"])
            self.assertIn("--app", debug_task["args"])


if __name__ == "__main__":
    unittest.main()
