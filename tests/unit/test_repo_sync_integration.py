"""
Tests for the integrated repo sync functionality in FileWatcherProcess.
"""

import tempfile
import unittest
from pathlib import Path

from fastled.filewatcher import FileWatcherProcess


class TestRepoSyncIntegration(unittest.TestCase):
    def setUp(self):
        """Set up test environment with temporary directory structure."""
        self.temp_dir = tempfile.mkdtemp()
        self.fastled_repo_dir = Path(self.temp_dir) / "fastled_repo"
        self.non_fastled_dir = Path(self.temp_dir) / "other_repo"
        
        # Create FastLED repo structure
        self.fastled_repo_dir.mkdir(parents=True)
        self.fastled_src_dir = self.fastled_repo_dir / "src"
        self.fastled_src_dir.mkdir(parents=True)
        
        # Create library.properties to make it look like FastLED repo
        library_props = self.fastled_repo_dir / "library.properties"
        library_props.write_text("name=FastLED\nversion=3.9.16\n")
        
        # Create some source files
        (self.fastled_src_dir / "test.h").write_text("// Test header\n#include <iostream>\n")
        (self.fastled_src_dir / "platforms").mkdir()
        (self.fastled_src_dir / "platforms" / "wasm").mkdir(parents=True)
        (self.fastled_src_dir / "platforms" / "wasm" / "test.cpp").write_text("// WASM code\nint main() {\n  return 0;\n}\n")
        
        # Create non-FastLED directory
        self.non_fastled_dir.mkdir(parents=True)
        (self.non_fastled_dir / "some_file.txt").write_text("not a fastled repo")
        
    def tearDown(self):
        """Clean up temporary directory."""
        import shutil
        shutil.rmtree(self.temp_dir)
        
    def test_auto_detects_fastled_src_directory(self):
        """Test that FileWatcherProcess auto-detects FastLED src directory and enables repo sync."""
        watcher = FileWatcherProcess(self.fastled_src_dir, [])
        
        try:
            # Should have automatically created repo sync cache
            self.assertIsNotNone(watcher.repo_sync_cache)
            self.assertGreater(watcher.repo_sync_cache.get_cached_file_count(), 0)
            
            # Should track the test files we created
            test_file_path = str(self.fastled_src_dir / "test.h")
            self.assertTrue(watcher.repo_sync_cache.is_file_tracked(test_file_path))
            
            wasm_file_path = str(self.fastled_src_dir / "platforms" / "wasm" / "test.cpp")
            self.assertTrue(watcher.repo_sync_cache.is_file_tracked(wasm_file_path))
            
        finally:
            watcher.stop()
            
    def test_auto_detects_fastled_repo_root(self):
        """Test that FileWatcherProcess auto-detects FastLED repo when watching repo root."""
        watcher = FileWatcherProcess(self.fastled_repo_dir, [])
        
        try:
            # Should have automatically created repo sync cache
            self.assertIsNotNone(watcher.repo_sync_cache)
            
        finally:
            watcher.stop()
            
    def test_no_repo_sync_for_non_fastled_directory(self):
        """Test that FileWatcherProcess doesn't enable repo sync for non-FastLED directories."""
        watcher = FileWatcherProcess(self.non_fastled_dir, [])
        
        try:
            # Should not have created repo sync cache
            self.assertIsNone(watcher.repo_sync_cache)
            
        finally:
            watcher.stop()


if __name__ == "__main__":
    unittest.main()