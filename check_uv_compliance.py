#!/usr/bin/env python3
"""
Script to check for direct python/python3 usage that should be replaced with 'uv run'
This script itself uses python3 in shebang as it's a validation tool.
"""

import os
import re
import sys
from pathlib import Path

def check_file_for_python_usage(file_path):
    """Check a file for direct python/python3 usage"""
    violations = []
    
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            lines = f.readlines()
    except (UnicodeDecodeError, PermissionError):
        return violations
    
    for i, line in enumerate(lines, 1):
        # Skip comments and strings that might contain "python" as text
        if line.strip().startswith('#'):
            continue
            
        # Look for command-line usage of python/python3
        if re.search(r'\b(python|python3)\s+\S', line):
            # Skip legitimate usage patterns
            if any(pattern in line for pattern in [
                'python-version',
                'setup-python',
                'requires-python',
                'Programming Language :: Python',
                'python.exe',
                'python3.exe',
                '/usr/bin/python',
                'env python',
                'import',
                'from',
                'python.autoComplete',
                'python.linting',
                'python.formatting',
                'python.testing',
                'python.analysis',
                'python.defaultInterpreterPath',
                'python.terminal',
                '.Python',
                'share/python-wheels',
                'ipython',
                '.python-version'
            ]):
                continue
                
            violations.append({
                'file': file_path,
                'line': i,
                'content': line.strip(),
                'suggestion': line.strip().replace('python3', 'uv run').replace('python', 'uv run')
            })
    
    return violations

def main():
    """Main function to check all relevant files"""
    root_dir = Path('.')
    violations = []
    
    # File patterns to check
    patterns = [
        '*.sh',
        '*.py',
        '*.md',
        '*.yml',
        '*.yaml',
        '*.json',
        '**/Dockerfile*',
        '**/*.sh',
        '**/*.py',
        '**/*.md'
    ]
    
    # Files to skip
    skip_files = {
        'check_uv_compliance.py',  # This file itself
        '.git',
        '__pycache__',
        'node_modules',
        '.venv',
        'venv',
        '.uv'
    }
    
    checked_files = set()
    
    for pattern in patterns:
        for file_path in root_dir.glob(pattern):
            if file_path.is_file() and str(file_path) not in checked_files:
                # Skip files in excluded directories
                if any(skip in str(file_path) for skip in skip_files):
                    continue
                    
                checked_files.add(str(file_path))
                file_violations = check_file_for_python_usage(str(file_path))
                violations.extend(file_violations)
    
    if violations:
        print("🚨 Found potential violations of uv usage policy:")
        print("=" * 60)
        
        for violation in violations:
            print(f"\nFile: {violation['file']}")
            print(f"Line {violation['line']}: {violation['content']}")
            print(f"Suggested: {violation['suggestion']}")
            print("-" * 40)
        
        print(f"\n📊 Total violations found: {len(violations)}")
        print("\n💡 Remember: Use 'uv run' instead of 'python' or 'python3'")
        return 1
    else:
        print("✅ No violations found! All Python usage appears compliant with uv policy.")
        return 0

if __name__ == "__main__":
    sys.exit(main())