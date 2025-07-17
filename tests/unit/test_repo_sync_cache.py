"""
Tests for the repo sync file cache functionality.
"""

import tempfile
import unittest
from pathlib import Path

from fastled.repo_sync_cache import RepoSyncFileCache


class TestRepoSyncFileCache(unittest.TestCase):
    def setUp(self):
        """Set up test environment with temporary directory structure."""
        self.temp_dir = tempfile.mkdtemp()
        self.fastled_src_dir = Path(self.temp_dir) / "src"
        self.fastled_src_dir.mkdir(parents=True)
        
        # Create mock directory structure
        (Path(self.temp_dir) / "src" / "platforms" / "wasm").mkdir(parents=True)
        (Path(self.temp_dir) / "src" / "platforms" / "stub").mkdir(parents=True)
        (Path(self.temp_dir) / "src" / "sensors").mkdir(parents=True)
        (Path(self.temp_dir) / "src" / "fx").mkdir(parents=True)
        (Path(self.temp_dir) / "src" / "sensor").mkdir(parents=True)
        (Path(self.temp_dir) / "src" / "thirdparty").mkdir(parents=True)
        
        # Create some test files
        self.test_files = {
            "src/test.h": "// Test header\n#include <iostream>\n",
            "src/platforms/wasm/test.cpp": "// WASM specific code\nint main() {\n  return 0;\n}\n",
            "src/platforms/stub/stub.h": "// Stub header\n#ifndef STUB_H\n#define STUB_H\n#endif\n",
            "src/sensors/sensor.cpp": "// Sensor code\nvoid read_sensor() {\n}\n",
            "src/fx/effect.h": "// Effect header\nclass Effect {\n};\n",
            "src/sensor/temp.cpp": "// Temperature sensor\nfloat read_temp() {\n  return 25.0;\n}\n",
            "src/thirdparty/lib.h": "// Third party library\nextern int lib_init();\n"
        }
        
        # Write test files
        for rel_path, content in self.test_files.items():
            file_path = Path(self.temp_dir) / rel_path
            file_path.write_text(content)
            
    def tearDown(self):
        """Clean up temporary directory."""
        import shutil
        shutil.rmtree(self.temp_dir)
        
    def test_load_all_files(self):
        """Test that all matching files are loaded into cache."""
        cache = RepoSyncFileCache(self.fastled_src_dir)
        cache.load_all_files()
        
        # Should have loaded all test files
        self.assertEqual(cache.get_cached_file_count(), len(self.test_files))
        
        # Check that specific files are tracked
        test_file_path = Path(self.temp_dir) / "src" / "test.h"
        self.assertTrue(cache.is_file_tracked(str(test_file_path)))
        
    def test_line_ending_normalization(self):
        """Test that different line endings are normalized to unix format."""
        cache = RepoSyncFileCache(self.fastled_src_dir)
        
        # Test normalization directly
        windows_content = "line1\r\nline2\r\nline3\r\n"
        mac_content = "line1\rline2\rline3\r"
        unix_content = "line1\nline2\nline3\n"
        
        normalized_windows = cache._normalize_line_endings(windows_content)
        normalized_mac = cache._normalize_line_endings(mac_content)
        normalized_unix = cache._normalize_line_endings(unix_content)
        
        # All should be normalized to unix format
        self.assertEqual(normalized_windows, unix_content)
        self.assertEqual(normalized_mac, unix_content)
        self.assertEqual(normalized_unix, unix_content)
        
    def test_spurious_change_detection(self):
        """Test that line ending changes are detected as spurious."""
        cache = RepoSyncFileCache(self.fastled_src_dir)
        cache.load_all_files()
        
        test_file_path = Path(self.temp_dir) / "src" / "test.h"
        original_content = test_file_path.read_text()
        
        # Write the same content but with Windows line endings
        windows_content = original_content.replace('\n', '\r\n')
        test_file_path.write_text(windows_content, newline='')
        
        # Should detect this as NOT a real change
        self.assertFalse(cache.has_file_actually_changed(str(test_file_path)))
        
    def test_real_change_detection(self):
        """Test that actual content changes are detected."""
        cache = RepoSyncFileCache(self.fastled_src_dir)
        cache.load_all_files()
        
        test_file_path = Path(self.temp_dir) / "src" / "test.h"
        
        # Write actually different content
        new_content = "// Modified header\n#include <iostream>\nint x = 42;\n"
        test_file_path.write_text(new_content)
        
        # Should detect this as a real change
        self.assertTrue(cache.has_file_actually_changed(str(test_file_path)))
        
    def test_untracked_file_behavior(self):
        """Test behavior with files not tracked by the cache."""
        cache = RepoSyncFileCache(self.fastled_src_dir)
        cache.load_all_files()
        
        # Create a file outside the tracked patterns
        untracked_file = Path(self.temp_dir) / "untracked.txt"
        untracked_file.write_text("untracked content")
        
        # Should not be tracked
        self.assertFalse(cache.is_file_tracked(str(untracked_file)))
        
        # Should be considered changed (since not tracked)
        self.assertTrue(cache.has_file_actually_changed(str(untracked_file)))


if __name__ == "__main__":
    unittest.main()