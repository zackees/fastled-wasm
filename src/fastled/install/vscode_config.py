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

    # FastLED repository-specific settings
    fastled_settings = {
        "clangd.arguments": [
            "--compile-commands-dir=${workspaceFolder}/compile_commands",
            "--header-insertion=never",
            "--clang-tidy",
            "--background-index",
        ],
        "C_Cpp.intelliSenseEngine": "disabled",
        "files.associations": {"*.ino": "cpp", "*.h": "cpp", "*.cpp": "cpp"},
        "editor.formatOnSave": True,
        "editor.formatOnType": True,
        "editor.tabSize": 4,
        "editor.insertSpaces": True,
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

    print(f"✅ Updated {settings_json_path} with FastLED development settings")
