# Task: Implement `fastled --install` Feature

## Overview

Implement the `fastled --install` command that provides a comprehensive setup for FastLED development environments. The command adapts its behavior based on the project context and user choices, ensuring safe installation while protecting existing development environments.

## Core Requirements

### 1. Project Detection and Validation

#### Directory Validation Flow
1. **Current Directory**: Check if `.vscode/` exists in current directory
2. **Parent Search**: If not found, search up to 5 parent directories for `.vscode/`
3. **IDE Availability**: Verify `code` (VSCode) OR `cursor` (Cursor) commands are available
4. **Project Generation**: Offer to create new VSCode project if none found and IDE available

#### Repository Type Detection
```python
def detect_fastled_project():
    """Check if library.json contains FastLED"""
    # Returns True if library.json has "name": "FastLED"

def is_fastled_repository():
    """🚨 CRITICAL: Detect actual FastLED repository"""
    # Strict verification of multiple markers:
    # - src/FastLED.h, examples/Blink/Blink.ino, ci/ci-compile.py
    # - src/platforms/, tests/test_*.cpp pattern
    # - library.json with correct name and repository URL
```

### 2. User Interaction Flow

#### Auto Debug Extension Prompt
```
Would you like to install the plugin for FastLED (auto-debug)? [y/n]
```
- **Always prompt** before installing extension
- **Dry-run mode**: Skip prompt, simulate installation
- **Installation**: Support both VSCode and Cursor

#### Project Generation Prompt
```
No .vscode directory found in current directory or parent directories.
Would you like to generate a VSCode project with FastLED configuration? [y/n]
```

#### Parent Directory Prompt
```
Found a .vscode project in /path/to/parent/
Install there? [y/n]
```

#### Examples Installation Prompt
```
No existing Arduino content found.
Would you like to install FastLED examples? [y/n]
```
- **Only prompt** if NO `.ino` files AND NO `examples/` folder exist
- **New projects**: Auto-install examples without prompting
- **Implementation**: Use FastLED's built-in `--project-init` command

### 3. Installation Modes

| Project Type | Detection Criteria | Installation Behavior |
|--------------|-------------------|----------------------|
| **Basic Project** | `.vscode/` exists, not FastLED | Arduino debugging + tasks |
| **External FastLED** | `library.json` has FastLED, not repository | Arduino debugging + tasks (NO clangd) |
| **FastLED Repository** | All repository markers present | Full development environment |
| **New Project** | No `.vscode/`, IDE available | Generate project + `--project-init` + tasks |

## 🚨 CRITICAL SAFETY REQUIREMENTS

### clangd Environment Protection
- **MANDATORY**: Only install clangd settings in actual FastLED repository
- **Detection**: Must pass strict `is_fastled_repository()` verification
- **Protection**: Prevents corruption of user's C++ development environment
- **NO EXCEPTIONS**: This rule cannot be bypassed under any circumstances

### Error Messages
- Missing `.vscode/` + No IDE: `"No supported IDE found (VSCode or Cursor). Please install VSCode or Cursor first."`
- clangd Protection: `"⚠️ Skipping clangd settings - not in FastLED repository (protects your environment)"`

## Implementation Specification

### Core Functions

#### 1. Main Installation Function
```python
def fastled_install(dry_run=False):
    """Main installation function with dry-run support"""
    try:
        # 1. Validate VSCode project or offer alternatives
        validate_vscode_project()
        
        # 2. Detect project type
        is_fastled_project = detect_fastled_project()
        is_repository = is_fastled_repository()
        
        # 3. Auto Debug extension (with prompt)
        if not dry_run:
            response = input("Would you like to install the plugin for FastLED (auto-debug)? [y/n]")
        else:
            response = 'yes'
            
        if response in ['y', 'yes']:
            if dry_run:
                print("[DRY-RUN]: NO PLUGIN INSTALLED")
            else:
                install_auto_debug_extension()
        
        # 4. Configure VSCode files
        update_launch_json_for_arduino()
        generate_fastled_tasks()
        
                 # 5. Examples installation (conditional)
         if not check_existing_arduino_content():
             install_fastled_examples_via_project_init()
        
        # 6. Full development setup (repository only)
        if is_fastled_project:
            if is_repository:
                setup_fastled_development_environment()
                update_vscode_settings_for_fastled()
            else:
                print("⚠️ Skipping clangd settings - not in FastLED repository")
        
        # 7. Post-installation auto-execution
        if not dry_run:
            auto_execute_fastled()
        
        return True
    except Exception as e:
        print(f"❌ Installation failed: {e}")
        return False
```

#### 2. VSCode Configuration Generation

**Launch Configuration (`launch.json`)**:
```json
{
    "name": "🎯 Auto Debug (Smart File Detection)",
    "type": "auto-debug",
    "request": "launch",
    "map": {
        "*.ino": "Arduino: Run .ino with FastLED",
        "*.py": "Python: Current File (UV)"
    }
}
```

**Build Tasks (`tasks.json`)**:
```json
{
    "label": "Run FastLED (Debug)",
    "command": "fastled",
    "args": ["${file}", "--debug"],
    "group": {"kind": "build", "isDefault": true}
},
{
    "label": "Run FastLED (Quick)", 
    "command": "fastled",
    "args": ["${file}", "--background-update"]
}
```

**Settings (`settings.json`)**:
- Basic projects: File associations and basic formatting
- FastLED repository: clangd configuration + IntelliSense overrides

#### 3. Examples Installation System

**Content Detection**:
```python
def check_existing_arduino_content():
    """Check for .ino files OR examples/ folder"""
    ino_files = list(Path.cwd().rglob("*.ino"))
    examples_folder = Path("examples").exists()
    return len(ino_files) > 0 or examples_folder

def install_fastled_examples_via_project_init(force=False):
    """Install FastLED examples using built-in --project-init command"""
    if not force:
        print("No existing Arduino content found.")
        print("Would you like to install FastLED examples? [y/n]")
        
        response = input().strip().lower()
        if response not in ['y', 'yes']:
            print("Skipping FastLED examples installation.")
            return False
    
    print("📦 Installing FastLED examples using project initialization...")
    
    try:
        import subprocess
        
        # Use FastLED's built-in project initialization
        result = subprocess.run(
            ["fastled", "--project-init"],
            check=True, 
            capture_output=True, 
            text=True,
            cwd="."
        )
        
        print("✅ FastLED project initialized successfully!")
        print("📁 Examples and project structure created")
        print("🚀 Quick start: Check for generated .ino files and press F5 to debug")
        
        return True
        
    except subprocess.CalledProcessError as e:
        print(f"⚠️  Warning: Failed to initialize FastLED project: {e}")
        print("You can manually run: fastled --project-init")
        return False
    except FileNotFoundError:
        print("⚠️  Warning: FastLED package not found. Please install it first:")
        print("    pip install fastled")
        print("Then run: fastled --project-init")
        return False
```

**Installation Process**:
1. Execute `fastled --project-init` to initialize project with examples
2. Uses FastLED's built-in project initialization functionality
3. Creates appropriate project structure and example files

#### 4. Post-Installation Auto-Execution

**Trigger Conditions**:
- Arduino content detected (`.ino` files OR examples)
- Not in dry-run mode

**Execution Process**:
```python
def auto_execute_fastled():
    """Auto-launch fastled after successful installation"""
    if check_existing_arduino_content():
        # Filter arguments: remove --install, --dry-run
        # Add current directory if no target specified
        # Call main() directly: equivalent to 'fastled .'
```

## Testing Requirements

### Dry-Run Mode Support
**Command**: `fastled --install --dry-run`

**Behavior**:
- Skip actual extension installation: Output `[DRY-RUN]: NO PLUGIN INSTALLED`
- Create all `.vscode/*.json` files for validation
- Skip auto-execution at end
- Default to 'yes' for all prompts

### Unit Test Suite

#### Required Test Functions
```python
def test_fastled_install_dry_run():
    """Comprehensive dry-run validation in temporary directory"""
    # Verify all .vscode files created and valid
    # Check Auto Debug configuration mapping
    # Validate FastLED tasks presence and arguments
    # Confirm project initialization (via --project-init)

def test_fastled_install_existing_vscode_project():
    """Test configuration merging with existing projects"""
    # Preserve existing launch.json configurations
    # Merge tasks without duplicates

def test_fastled_install_auto_execution():
    """Verify auto-execution triggers correctly"""
    # Mock main() function call
    # Verify argument filtering

def test_fastled_repository_detection_safety():
    """🚨 CRITICAL: Test clangd protection"""
    # Create fake FastLED project
    # Verify clangd settings NOT applied
    # Confirm safety messages displayed
```

### Validation Criteria
1. **JSON Validity**: All generated `.vscode/*.json` files must be valid JSON
2. **Configuration Completeness**: Required configurations present
3. **Task Functionality**: Correct commands and arguments
4. **Safety Protection**: clangd settings only in repository
5. **Auto-Execution**: Proper argument filtering and main() call

## Installation Behavior Reference

### Complete Decision Matrix

| Current Dir | Parent Search | IDE Available | FastLED Repo | External FastLED | Has Content | Result |
|-------------|---------------|---------------|--------------|------------------|-------------|---------|
| ✅ .vscode | N/A | ✅ | ❌ | ❌ | Any | **Basic**: Arduino debugging |
| ✅ .vscode | N/A | ✅ | ❌ | ✅ | Any | **Limited**: Arduino debugging (NO clangd) |
| ✅ .vscode | N/A | ✅ | ✅ | ✅ | Any | **Full Dev**: Arduino + clangd + dev env |
| ❌ | ✅ Found | ✅ | Any | Any | Any | **Prompt**: Install in parent? |
| ❌ | ❌ None | ✅ | Any | Any | Any | **Generate**: New project + `--project-init` |
| ❌ | ❌ None | ❌ | Any | Any | Any | **Error**: No IDE found |

### Post-Installation Actions
- **Examples**: Installed via `fastled --project-init` if no existing Arduino content
- **Auto-Execution**: Runs `fastled .` if content present (skip in dry-run)
- **Protection Messages**: Clear explanations when skipping clangd

## Success Criteria

### Core Functionality
1. ✅ Validates VSCode project directory or searches parent directories
2. ✅ Offers to install in found parent VSCode project with directory change
3. ✅ Checks for available IDE before offering project generation
4. ✅ Generates complete VSCode project when requested
5. ✅ Prompts user before installing Auto Debug extension
6. ✅ Downloads and installs extension only with user consent

### Configuration Management
7. ✅ Updates `.vscode/launch.json` for Arduino debugging
8. ✅ Generates FastLED build tasks in `.vscode/tasks.json`
9. ✅ Merges with existing configurations without conflicts
10. ✅ Supports `.ino` files anywhere in project

### Content Management
11. ✅ Detects existing Arduino content to avoid conflicts
12. ✅ Prompts for examples only when no existing content found
13. ✅ Uses FastLED's built-in `--project-init` for examples installation
14. ✅ Creates appropriate project structure and example files

### Safety and Environment Protection
15. ✅ 🚨 **CRITICAL**: Only applies clangd settings in actual FastLED repository
16. ✅ 🚨 **CRITICAL**: Protects user environments from FastLED configurations
17. ✅ Detects FastLED repository with multiple verification markers
18. ✅ Provides clear protection messages

### Advanced Features
19. ✅ Auto-executes FastLED when Arduino content present
20. ✅ Skips auto-execution in dry-run mode
21. ✅ Properly filters arguments for auto-execution
22. ✅ Handles all error conditions gracefully

### Testing and Quality
23. ✅ Supports comprehensive dry-run mode for testing
24. ✅ Validates all generated JSON configurations
25. ✅ Provides clear feedback and manual fallback instructions

## Implementation Plan

### File Structure Organization

All implementation files must be located in `src/fastled-wasm/install/` directory:

```
src/fastled-wasm/install/
├── __init__.py
├── main.py              # Main installation orchestrator
├── project_detection.py # Project and repository detection logic
├── vscode_config.py     # VSCode configuration generation
├── examples_manager.py  # Examples installation via --project-init
├── extension_manager.py # Auto Debug extension handling
└── test_install.py      # Single comprehensive test file
```

### Implementation Approach

**Minimal Changes Strategy**: Make the smallest possible changes to existing codebase while implementing full functionality.

#### Core Files

**1. `main.py`** - Installation orchestrator
```python
def fastled_install(dry_run=False):
    """Main installation function - coordinates all installation steps"""
    # Entry point that calls other modules in sequence

def auto_execute_fastled():
    """Post-installation auto-execution"""
    # Handles argument filtering and main() call
```

**2. `project_detection.py`** - Detection logic
```python
def validate_vscode_project():
    """VSCode project validation and generation flow"""

def find_vscode_project_upward(max_levels=5):
    """Search parent directories for .vscode"""

def detect_fastled_project():
    """Check library.json for FastLED"""

def is_fastled_repository():
    """🚨 CRITICAL: Actual FastLED repository detection"""

def check_existing_arduino_content():
    """Detect existing .ino files or examples/"""
```

**3. `vscode_config.py`** - VSCode configuration
```python
def generate_vscode_project():
    """Create complete .vscode structure"""

def update_launch_json_for_arduino():
    """Update launch.json with Arduino debugging"""

def generate_fastled_tasks():
    """Generate/update tasks.json with FastLED tasks"""

def update_vscode_settings_for_fastled():
    """🚨 Repository-only: Apply clangd settings"""
```

**4. `examples_manager.py`** - Examples via --project-init
```python
def install_fastled_examples_via_project_init(force=False):
    """Use fastled --project-init for examples"""
```

**5. `extension_manager.py`** - Auto Debug extension
```python
def download_auto_debug_extension():
    """Download .vsix from GitHub"""

def install_vscode_extensions(extension_path):
    """Install in VSCode/Cursor"""
```

### Testing Requirements

**Single Test File**: `test_install.py` with **maximum 15 tests** running in **~3 seconds total**.

#### Test Strategy
```python
# Mock Strategy:
# - Mock repository/IDE detection functions 
# - Allow actual file writes to temporary directories
# - --dry-run prevents writes (separate from unit test mocks)

@patch('project_detection.is_fastled_repository')
@patch('project_detection.shutil.which')
class TestFastLEDInstall(unittest.TestCase):
    
    def test_dry_run_basic_project(self, mock_which, mock_repo):
        """Test 1: Dry-run in basic project"""
        
    def test_dry_run_fastled_external(self, mock_which, mock_repo):
        """Test 2: Dry-run in external FastLED project"""
        
    def test_dry_run_fastled_repository(self, mock_which, mock_repo):
        """Test 3: Dry-run in actual FastLED repository"""
        
    def test_existing_vscode_project(self, mock_which, mock_repo):
        """Test 4: Merge with existing .vscode configs"""
        
    def test_parent_directory_detection(self, mock_which, mock_repo):
        """Test 5: Find .vscode in parent directories"""
        
    def test_project_generation(self, mock_which, mock_repo):
        """Test 6: Generate new VSCode project"""
        
    def test_arduino_content_detection(self, mock_which, mock_repo):
        """Test 7: Detect existing .ino files"""
        
    def test_tasks_json_merging(self, mock_which, mock_repo):
        """Test 8: Merge FastLED tasks with existing"""
        
    def test_launch_json_updates(self, mock_which, mock_repo):
        """Test 9: Update launch.json configurations"""
        
    def test_safety_clangd_protection(self, mock_which, mock_repo):
        """Test 10: 🚨 CRITICAL - clangd safety protection"""
        
    def test_auto_execution_trigger(self, mock_which, mock_repo):
        """Test 11: Post-installation auto-execution"""
        
    def test_no_ide_error_handling(self, mock_which, mock_repo):
        """Test 12: Error when no IDE available"""
        
    def test_examples_installation(self, mock_which, mock_repo):
        """Test 13: --project-init examples installation"""
        
    def test_extension_installation_flow(self, mock_which, mock_repo):
        """Test 14: Auto Debug extension prompt/install"""
        
    def test_comprehensive_integration(self, mock_which, mock_repo):
        """Test 15: End-to-end integration test"""
```

#### Mock Configuration
```python
# Mock repository detection but allow file I/O
mock_repo.return_value = False  # or True for repository tests
mock_which.side_effect = lambda cmd: '/usr/bin/code' if cmd == 'code' else None

# Each test runs in isolated tempfile.TemporaryDirectory()
# Real file writes happen, but to temporary locations
# Validates actual JSON generation and file structure
```

#### Performance Requirements
- **Total runtime**: ~3 seconds for all 15 tests
- **Individual test**: <200ms average
- **Fast mocking**: Repository/IDE detection mocked for speed
- **Real I/O**: File writes to temp directories (validates actual output)
- **Dry-run separation**: `--dry-run` flag separate from unit test mocks

### CLI Integration

**Minimal CLI Changes**: Add to existing fastled-wasm argument parser
```python
# In existing CLI handler
if args.install:
    from src.fastled_wasm.install.main import fastled_install
    result = fastled_install(dry_run=args.dry_run)
    sys.exit(0 if result else 1)
```

### Integration Notes

- **CLI Interface**: Add to existing fastled-wasm command structure
- **Command Pattern**: Follow existing argument parsing and error handling
- **Compatibility**: Maintain compatibility with existing fastled commands
- **Workflow Consistency**: Align with current FastLED development patterns
- **Test Integration**: Include in automated test suite with proper isolation
- **Minimal Impact**: Changes isolated to `src/fastled-wasm/install/` directory





#################### SECOND FEATURE REQUEST #########################


# FastLED Debug Feature Requirements

## Overview

This document outlines the requirements for enhancing the `fastled --debug` feature to automatically enable app mode when Playwright cache is available, and to integrate with the `fastled --install` VSCode task generation.

## Current State

Based on the conversation summary and codebase analysis, the current FastLED ecosystem includes:

1. **FastLED C++ Library** (this repository) - Core LED control library
2. **FastLED Python CLI** (external PyPI package) - Web compiler and development tools
3. **VSCode Integration** - Tasks and debugging configurations

## Feature Requirements

### 1. Playwright Cache Detection for `fastled --debug`

**Requirement**: When `fastled --debug` is used, check for Playwright cache and automatically enable app mode.

**Behavior**:
- **If `~/.fastled/playwright` exists**: Automatically add `--app` flag without prompting
- **If `~/.fastled/playwright` does NOT exist**: Prompt user "Would you like to install the FastLED debugger? [y/n]"

**Implementation Location**: External FastLED Python CLI package (not this repository)

### 2. VSCode Tasks Integration

**Requirement**: Update VSCode tasks generation to always include `--app` flag for debug mode.

**Current Tasks** (from conversation summary):
- "Run FastLED (Debug)" - Should use `--debug --app` flags
- "Run FastLED (Quick)" - Should use `--background-update` flag

**Implementation**: When `fastled --install` generates `.vscode/tasks.json`, ensure debug task includes:
```json
{
    "label": "Run FastLED (Debug)",
    "command": "fastled",
    "args": ["${file}", "--debug", "--app"],
    // ... other task configuration
}
```

### 3. Auto-App Mode Logic

**Condition for Automatic `--app`**:
```
IF (Playwright cache exists at ~/.fastled/playwright) THEN
    Add --app flag automatically
ELSE
    Prompt user: "Would you like to install the FastLED debugger? [y/n]"
    IF user says yes THEN
        Install playwright and add --app flag
    ELSE
        Run without --app flag
    END IF
END IF
```

### 4. Integration with Existing Install Process

**From conversation summary**, the `fastled --install` feature already:
- Checks for `.vscode/` directory existence
- Searches parent directories for existing VSCode projects
- Prompts for auto-debug extension installation
- Generates VSCode configuration files

**New Requirement**: Integrate Playwright cache detection into this flow.

## Technical Implementation Notes

### Playwright Cache Location
- **Path**: `~/.fastled/playwright`
- **Detection**: Check if directory exists and contains browser installation
- **Creation**: Installed via `pip install playwright && python -m playwright install`

### VSCode Tasks Template
The generated tasks should include:

```json
{
    "version": "2.0.0",
    "tasks": [
        {
            "type": "shell",
            "label": "Run FastLED (Debug)",
            "command": "fastled",
            "args": ["${file}", "--debug", "--app"],
            "options": {
                "cwd": "${workspaceFolder}"
            },
            "group": "build",
            "presentation": {
                "echo": true,
                "reveal": "always",
                "focus": true,
                "panel": "new",
                "showReuseMessage": false,
                "clear": true
            },
            "detail": "Run FastLED with debug mode and app visualization",
            "problemMatcher": []
        },
        {
            "type": "shell",
            "label": "Run FastLED (Quick)",
            "command": "fastled", 
            "args": ["${file}", "--background-update"],
            "options": {
                "cwd": "${workspaceFolder}"
            },
            "group": "build",
            "presentation": {
                "echo": true,
                "reveal": "always", 
                "focus": true,
                "panel": "new",
                "showReuseMessage": false,
                "clear": true
            },
            "detail": "Run FastLED with quick background update mode",
            "problemMatcher": []
        }
    ]
}
```

## Dependencies

### External Packages Required
- `fastled` (PyPI package) - Contains the CLI implementation
- `playwright` (PyPI package) - Browser automation for app mode
- VSCode or Cursor - IDE for task execution

### File System Requirements
- Write access to `~/.fastled/` directory
- Write access to project `.vscode/` directory
- Read access to check Playwright cache existence

## Backward Compatibility

### Existing Behavior Preservation
- If user manually specifies `--debug` without `--app`, respect that choice
- Existing `fastled --install` functionality should continue to work
- Non-interactive environments should not prompt

### Graceful Degradation
- If Playwright is not available, continue without app mode
- If VSCode is not available, skip task generation
- If write permissions are lacking, warn but continue

## User Experience Flow

### First-Time User (No Playwright)
1. User runs `fastled --debug sketch.ino`
2. System detects no Playwright cache
3. Prompt: "Would you like to install the FastLED debugger? [y/n]"
4. If yes: Install Playwright, add `--app` flag, continue
5. If no: Run without `--app` flag

### Returning User (Has Playwright)
1. User runs `fastled --debug sketch.ino`
2. System detects Playwright cache at `~/.fastled/playwright`
3. Automatically add `--app` flag
4. Run with debug app mode enabled

### VSCode Task Usage
1. User opens `.ino` file in VSCode
2. Run "Run FastLED (Debug)" task
3. Task executes `fastled sketch.ino --debug --app`
4. Debug session starts with web app visualization

## Testing Requirements

### Unit Tests Needed
- Playwright cache detection logic
- Task generation with correct flags
- User prompt handling (interactive/non-interactive modes)
- Error handling for missing dependencies

### Integration Tests Needed
- End-to-end `fastled --debug` with and without Playwright
- VSCode task execution in various environments
- Cross-platform behavior (Windows, macOS, Linux)

## Implementation Priority

1. **High Priority**: Playwright cache detection for `fastled --debug`
2. **High Priority**: VSCode tasks generation with `--app` flag
3. **Medium Priority**: User prompting for debugger installation
4. **Low Priority**: Advanced error handling and edge cases

## Notes

- This feature enhancement is for the external FastLED Python CLI package
- The FastLED C++ library (this repository) does not need modification
- Implementation should be backward compatible with existing workflows
- Consider adding a `--no-app` flag to explicitly disable app mode if needed
