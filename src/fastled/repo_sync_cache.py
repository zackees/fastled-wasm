"""
Repo sync file cache for preventing spurious recompilation notifications.

When repo syncing is active, this module loads source files into memory with
normalized unix line endings and compares changed files against the cached
versions to avoid unnecessary recompilations due to line ending differences.
"""

import glob
import os
from pathlib import Path
from typing import Dict, List, Set

_PATTERNS = [

    "src/*.{c,cpp,h,hpp}",
    "src/platforms/wasm/**/*.{c,cpp,h,hpp}",
    "src/platforms/shared/**/*.{c,cpp,h,hpp}",
    "src/platforms/stub/**/*.{c,cpp,h,hpp}", 
    "src/sensors/**/*.{c,cpp,h,hpp}",
    "src/fx/**/*.{c,cpp,h,hpp}",    
    "src/sensor/**/*.{c,cpp,h,hpp}",
    "src/thirdparty/**/*.{c,cpp,h,hpp}"

]


class RepoSyncFileCache:
    """
    Caches source files in memory with normalized line endings to prevent
    spurious recompilation notifications when files haven't actually changed.
    """
    
    def __init__(self, fastled_src_dir: Path):
        """
        Initialize the file cache for the given FastLED source directory.
        
        Args:
            fastled_src_dir: Path to the FastLED src directory
        """
        self.fastled_src_dir = fastled_src_dir
        self.file_cache: Dict[str, str] = {}
        self.patterns = _PATTERNS
        
    def _normalize_line_endings(self, content: str) -> str:
        """Convert content to unix line endings."""
        return content.replace('\r\n', '\n').replace('\r', '\n')
        
    def _load_file_content(self, file_path: Path) -> str:
        """Load file content and normalize line endings."""
        try:
            with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                content = f.read()
            return self._normalize_line_endings(content)
        except Exception:
            # If we can't read the file, return empty string
            return ""
            
    def _get_matching_files(self) -> Set[str]:
        """Get all files matching the repo sync patterns."""
        matching_files = set()
        
        # Change to the FastLED repo directory to make patterns work correctly
        original_cwd = os.getcwd()
        try:
            os.chdir(self.fastled_src_dir.parent)  # FastLED repo root
            
            for pattern in self.patterns:
                # Use glob to find files matching the pattern
                files = glob.glob(pattern, recursive=True)
                for file_path in files:
                    # Convert to absolute path and add to set
                    abs_path = (Path(self.fastled_src_dir.parent) / file_path).resolve()
                    if abs_path.is_file():
                        matching_files.add(str(abs_path))
                        
        finally:
            os.chdir(original_cwd)
            
        return matching_files
        
    def load_all_files(self) -> None:
        """Load all matching source files into memory cache."""
        print("Loading FastLED source files into memory cache for repo sync...")
        
        matching_files = self._get_matching_files()
        
        loaded_count = 0
        for file_path_str in matching_files:
            file_path = Path(file_path_str)
            content = self._load_file_content(file_path)
            self.file_cache[file_path_str] = content
            loaded_count += 1
            
        print(f"Loaded {loaded_count} source files into repo sync cache")
        
    def has_file_actually_changed(self, file_path: str) -> bool:
        """
        Check if a file has actually changed by comparing normalized content.
        
        Args:
            file_path: Absolute path to the file that changed
            
        Returns:
            True if the file content has actually changed, False if it's just
            a spurious notification (e.g., line ending differences)
        """
        file_path = str(Path(file_path).resolve())
        
        # If the file isn't in our cache, consider it changed
        if file_path not in self.file_cache:
            return True
            
        # Load current file content and normalize
        current_content = self._load_file_content(Path(file_path))
        cached_content = self.file_cache[file_path]
        
        # If content is the same, it's a spurious notification
        if current_content == cached_content:
            return False
            
        # Content has actually changed, update cache and return True
        self.file_cache[file_path] = current_content
        return True
        
    def is_file_tracked(self, file_path: str) -> bool:
        """
        Check if a file is tracked by the repo sync cache.
        
        Args:
            file_path: Absolute path to check
            
        Returns:
            True if the file is tracked by repo sync
        """
        file_path = str(Path(file_path).resolve())
        return file_path in self.file_cache
        
    def get_cached_file_count(self) -> int:
        """Get the number of files currently cached."""
        return len(self.file_cache)