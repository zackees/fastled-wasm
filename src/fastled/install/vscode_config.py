"""VSCode configuration generation for FastLED projects."""

import json
from pathlib import Path


def update_launch_json_for_arduino() -> None:
    """Update launch.json with Arduino debugging configuration."""
    launch_json_path = Path.cwd() / ".vscode" / "launch.json"

    # Default launch configuration
    arduino_config = {
        "name": "🎯 Auto Debug (Smart File Detection)",
        "type": "auto-debug",
        "request": "launch",
        "map": {
            "*.ino": "Arduino: Run .ino with FastLED",
            "*.py": "Python: Current File (UV)",
        },
    }

    if launch_json_path.exists():
        # Merge with existing
        try:
            with open(launch_json_path, "r") as f:
                data = json.load(f)
        except json.JSONDecodeError:
            data = {"version": "0.2.0", "configurations": []}
    else:
        data = {"version": "0.2.0", "configurations": []}

    # Check if configuration already exists
    configs = data.get("configurations", [])
    exists = any(cfg.get("name") == arduino_config["name"] for cfg in configs)

    if not exists:
        configs.insert(0, arduino_config)  # Add at the beginning
        data["configurations"] = configs

    # Write back
    launch_json_path.parent.mkdir(exist_ok=True)
    with open(launch_json_path, "w") as f:
        json.dump(data, f, indent=4)

    print(f"✅ Updated {launch_json_path}")


def generate_fastled_tasks() -> None:
    """Generate/update tasks.json with FastLED build tasks."""
    tasks_json_path = Path.cwd() / ".vscode" / "tasks.json"

    # FastLED tasks
    fastled_tasks = [
        {
            "type": "shell",
            "label": "Run FastLED (Debug)",
            "command": "fastled",
            "args": ["${file}", "--debug", "--app"],
            "options": {"cwd": "${workspaceFolder}"},
            "group": {"kind": "build", "isDefault": True},
            "presentation": {
                "echo": True,
                "reveal": "always",
                "focus": True,
                "panel": "new",
                "showReuseMessage": False,
                "clear": True,
            },
            "detail": "Run FastLED with debug mode and app visualization",
            "problemMatcher": [],
        },
        {
            "type": "shell",
            "label": "Run FastLED (Quick)",
            "command": "fastled",
            "args": ["${file}", "--background-update"],
            "options": {"cwd": "${workspaceFolder}"},
            "group": "build",
            "presentation": {
                "echo": True,
                "reveal": "always",
                "focus": True,
                "panel": "new",
                "showReuseMessage": False,
                "clear": True,
            },
            "detail": "Run FastLED with quick background update mode",
            "problemMatcher": [],
        },
    ]

    if tasks_json_path.exists():
        # Merge with existing
        try:
            with open(tasks_json_path, "r") as f:
                data = json.load(f)
        except json.JSONDecodeError:
            data = {"version": "2.0.0", "tasks": []}
    else:
        data = {"version": "2.0.0", "tasks": []}

    # Get existing tasks
    existing_tasks = data.get("tasks", [])
    existing_labels = {task.get("label") for task in existing_tasks}

    # Add new tasks if they don't exist
    for task in fastled_tasks:
        if task["label"] not in existing_labels:
            existing_tasks.append(task)

    data["tasks"] = existing_tasks

    # Write back
    tasks_json_path.parent.mkdir(exist_ok=True)
    with open(tasks_json_path, "w") as f:
        json.dump(data, f, indent=4)

    print(f"✅ Updated {tasks_json_path}")


def update_vscode_settings_for_fastled() -> None:
    """
    🚨 Repository-only: Apply clangd settings and IntelliSense overrides.
    This should ONLY be called for the actual FastLED repository.
    """
    from .project_detection import is_fastled_repository

    # Safety check - only apply in actual repository
    if not is_fastled_repository():
        return
    settings_json_path = Path.cwd() / ".vscode" / "settings.json"

    # FastLED repository-specific settings - updated to match the official FastLED repo
    fastled_settings = {
        # Terminal configuration
        "terminal.integrated.defaultProfile.windows": "Git Bash",
        "terminal.integrated.shellIntegration.enabled": False,
        "terminal.integrated.profiles.windows": {
            "Command Prompt": {"path": "C:\\Windows\\System32\\cmd.exe"},
            "Git Bash": {
                "path": "C:\\Program Files\\Git\\bin\\bash.exe",
                "args": ["--cd=."],
            },
        },
        # File settings
        "files.eol": "\n",  # Unix line endings
        "files.autoDetectEol": False,  # Prevent VS Code from auto-detecting and changing EOL
        "files.insertFinalNewline": True,  # Ensure files end with a newline
        "files.trimFinalNewlines": True,  # Remove extra newlines at the end
        "editor.tabSize": 4,
        "editor.insertSpaces": True,
        "editor.detectIndentation": True,
        "editor.formatOnSave": False,  # Disabled to prevent conflicts
        # Debugger defaults - ensure C++ debugger is used for C++ files
        "debug.defaultDebuggerType": "cppdbg",
        "debug.toolBarLocation": "docked",
        "debug.console.fontSize": 14,
        "debug.console.lineHeight": 19,
        # Python configuration (using uv as per project rules)
        "python.defaultInterpreterPath": "uv",
        "python.debugger": "debugpy",
        # File associations for debugger
        "[cpp]": {
            "editor.defaultFormatter": "llvm-vs-code-extensions.vscode-clangd",
            "debug.defaultDebuggerType": "cppdbg",
        },
        "[c]": {
            "editor.defaultFormatter": "ms-vscode.cpptools",
            "debug.defaultDebuggerType": "cppdbg",
        },
        "[ino]": {
            "editor.defaultFormatter": "ms-vscode.cpptools",
            "debug.defaultDebuggerType": "cppdbg",
        },
        # clangd configuration - enhanced
        "clangd.arguments": [
            "--compile-commands-dir=${workspaceFolder}",
            "--clang-tidy",
            "--header-insertion=never",
            "--completion-style=detailed",
            "--function-arg-placeholders=false",
            "--background-index",
            "--pch-storage=memory",
        ],
        "clangd.fallbackFlags": [
            "-std=c++17",
            "-I${workspaceFolder}/src",
            "-I${workspaceFolder}/tests",
            "-Wno-global-constructors",
        ],
        # Disable conflicting IntelliSense to let clangd handle C++ analysis
        "C_Cpp.intelliSenseEngine": "disabled",
        "C_Cpp.autocomplete": "disabled",
        "C_Cpp.errorSquiggles": "disabled",
        "C_Cpp.suggestSnippets": False,
        "C_Cpp.intelliSenseEngineFallback": "disabled",
        "C_Cpp.autocompleteAddParentheses": False,
        "C_Cpp.formatting": "disabled",
        "C_Cpp.vcpkg.enabled": False,
        "C_Cpp.configurationWarnings": "disabled",
        "C_Cpp.intelliSenseCachePath": "",
        "C_Cpp.intelliSenseCacheSize": 0,
        "C_Cpp.intelliSenseUpdateDelay": 0,
        "C_Cpp.workspaceParsingPriority": "lowest",
        "C_Cpp.disabled": True,
        # File associations - comprehensive
        "files.associations": {
            "*.ino": "cpp",
            "*.h": "cpp",
            "*.hpp": "cpp",
            "*.cpp": "cpp",
            "*.c": "c",
            "*.inc": "cpp",
            "*.tcc": "cpp",
            "*.embeddedhtml": "html",
            # Core C++ standard library files
            "compare": "cpp",
            "type_traits": "cpp",
            "cmath": "cpp",
            "limits": "cpp",
            "iostream": "cpp",
            "random": "cpp",
            "functional": "cpp",
            "bit": "cpp",
            "vector": "cpp",
            "array": "cpp",
            "string": "cpp",
            "memory": "cpp",
            "algorithm": "cpp",
            "iterator": "cpp",
            "utility": "cpp",
            "optional": "cpp",
            "variant": "cpp",
            "numeric": "cpp",
            "chrono": "cpp",
            "thread": "cpp",
            "mutex": "cpp",
            "atomic": "cpp",
            "future": "cpp",
            "condition_variable": "cpp",
        },
        # Disable Java language support and popups
        "java.enabled": False,
        "java.jdt.ls.enabled": False,
        "java.compile.nullAnalysis.mode": "disabled",
        "java.configuration.checkProjectSettingsExclusions": False,
        "java.import.gradle.enabled": False,
        "java.import.maven.enabled": False,
        "java.autobuild.enabled": False,
        "java.maxConcurrentBuilds": 0,
        "java.recommendations.enabled": False,
        "java.help.showReleaseNotes": False,
        "redhat.telemetry.enabled": False,
        # Java exclusions
        "java.project.sourcePaths": [],
        "java.project.referencedLibraries": [],
        "files.exclude": {
            "**/.classpath": True,
            "**/.project": True,
            "**/.factorypath": True,
        },
        # Disable PlatformIO auto-detection for .ino files in FastLED project
        "platformio.disableToolchainAutoInstaller": True,
        "platformio-ide.autoRebuildAutocompleteIndex": False,
        "platformio-ide.activateProjectOnTextEditorChange": False,
        "platformio-ide.autoOpenPlatformIOIniFile": False,
        "platformio-ide.autoPreloadEnvTasks": False,
        "platformio-ide.autoCloseSerialMonitor": False,
        "platformio-ide.disablePIOHomeStartup": True,
        # Disable conflicting extensions
        "extensions.ignoreRecommendations": True,
        # Semantic token color customizations for better C++ development
        "editor.semanticTokenColorCustomizations": {
            "rules": {
                # Types (classes, structs, enums) - Teal/Cyan
                "class": "#4EC9B0",
                "struct": "#4EC9B0",
                "type": "#4EC9B0",
                "enum": "#4EC9B0",
                "enumMember": "#B5CEA8",
                "typedef": "#4EC9B0",
                # Variables - Almost pure white for maximum readability
                "variable": "#FAFAFA",
                "variable.local": "#FAFAFA",
                # Parameters - Orange for clear distinction
                "parameter": "#FF8C42",
                "variable.parameter": "#FF8C42",
                # Properties - Light purple/pink
                "property": "#D197D9",
                # Functions and methods - Yellow
                "function": "#DCDCAA",
                "method": "#DCDCAA",
                "function.declaration": "#DCDCAA",
                "method.declaration": "#DCDCAA",
                # Namespaces - Soft blue
                "namespace": "#86C5F7",
                # Constants and readonly - Light green with italic
                "variable.readonly": {"foreground": "#B5CEA8", "fontStyle": "italic"},
                "variable.defaultLibrary": "#B5CEA8",
                # Macros and defines - Muted red/salmon
                "macro": "#E06C75",
                # String literals - Peach/salmon
                "string": "#CE9178",
                # Numbers - Light green
                "number": "#B5CEA8",
                # Keywords - Pink/magenta
                "keyword": "#C586C0",
                # Storage specifiers - Bright magenta/pink
                "keyword.storage": "#FF79C6",
                "storageClass": "#FF79C6",
                # Built-in types - Different from user-defined types
                "type.builtin": "#569CD6",
                "keyword.type": "#569CD6",
                # Comments - Green
                "comment": "#6A9955",
                "comment.documentation": "#6A9955",
            }
        },
        # Inlay hints - Brighter gray for better visibility
        "editor.inlayHints.fontColor": "#808080",
        "editor.inlayHints.background": "#3C3C3C20",
    }

    if settings_json_path.exists():
        # Merge with existing
        try:
            with open(settings_json_path, "r") as f:
                data = json.load(f)
        except json.JSONDecodeError:
            data = {}
    else:
        data = {}

    # Update settings
    data.update(fastled_settings)

    # Write back
    settings_json_path.parent.mkdir(exist_ok=True)
    with open(settings_json_path, "w") as f:
        json.dump(data, f, indent=4)

    print(
        f"✅ Updated {settings_json_path} with comprehensive FastLED development settings"
    )
