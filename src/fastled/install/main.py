"""Main installation orchestrator for FastLED --install feature."""

import sys

from .examples_manager import install_fastled_examples_via_project_init
from .extension_manager import install_auto_debug_extension
from .project_detection import (
    check_existing_arduino_content,
    detect_fastled_project,
    is_fastled_repository,
    validate_vscode_project,
)
from .vscode_config import (
    generate_fastled_tasks,
    update_launch_json_for_arduino,
    update_vscode_settings_for_fastled,
)


def fastled_install(dry_run: bool = False, no_interactive: bool = False) -> bool:
    """
    Main installation function with dry-run support.

    Args:
        dry_run: If True, simulate installation without making changes
        no_interactive: If True, fail instead of prompting for input

    Returns:
        True if installation successful, False otherwise
    """
    try:
        print("🚀 Starting FastLED installation...")

        # 1. Validate VSCode project or offer alternatives
        if not validate_vscode_project(no_interactive):
            return False

        # 2. Detect project type
        is_fastled_project = detect_fastled_project()
        is_repository = is_fastled_repository()

        if is_fastled_project:
            if is_repository:
                print(
                    "✅ Detected FastLED repository - will configure full development environment"
                )
            else:
                print(
                    "✅ Detected external FastLED project - will configure Arduino environment"
                )
        else:
            print(
                "✅ Detected standard project - will configure basic Arduino environment"
            )

        # 3. Auto Debug extension (with prompt)
        if not dry_run and not no_interactive:
            answer = (
                input(
                    "\nWould you like to install the plugin for FastLED (auto-debug)? [y/n] "
                )
                .strip()
                .lower()
            )
        elif no_interactive:
            print(
                "\n⚠️  Skipping Auto Debug extension installation in non-interactive mode"
            )
            answer = "no"
        else:
            answer = "yes"
            print("\n[DRY-RUN]: Simulating Auto Debug extension installation...")

        if answer in ["y", "yes"]:
            if not install_auto_debug_extension(dry_run):
                print(
                    "⚠️  Warning: Auto Debug extension installation failed, continuing..."
                )

        # 4. Configure VSCode files
        print("\n📝 Configuring VSCode files...")
        update_launch_json_for_arduino()
        generate_fastled_tasks()

        # 5. Examples installation (conditional)
        if not check_existing_arduino_content():
            if no_interactive:
                print(
                    "⚠️  No Arduino content found. In non-interactive mode, skipping examples installation."
                )
                print("    Run 'fastled --project-init' manually to install examples.")
            else:
                install_fastled_examples_via_project_init(no_interactive=no_interactive)
        else:
            print(
                "✅ Existing Arduino content detected, skipping examples installation"
            )

        # 6. Full development setup (repository only)
        if is_fastled_project:
            if is_repository:
                print("\n🔧 Setting up FastLED development environment...")
                update_vscode_settings_for_fastled()
            else:
                print(
                    "\n⚠️  Skipping clangd settings - not in FastLED repository (protects your environment)"
                )

        # 7. Post-installation auto-execution
        if not dry_run:
            auto_execute_fastled()
        else:
            print("\n[DRY-RUN]: Skipping auto-execution")

        print("\n✅ FastLED installation completed successfully!")
        return True

    except Exception as e:
        print(f"\n❌ Installation failed: {e}")
        import traceback

        traceback.print_exc()
        return False


def auto_execute_fastled() -> None:
    """Auto-launch fastled after successful installation."""
    if check_existing_arduino_content():
        print("\n🚀 Auto-launching FastLED...")

        # Import the main function to avoid circular imports
        from fastled.app import main

        # Filter out --install and --dry-run from sys.argv
        original_argv = sys.argv.copy()
        filtered_argv = [
            arg for arg in sys.argv if arg not in ["--install", "--dry-run"]
        ]

        # If no directory specified, add current directory
        if len(filtered_argv) == 1:  # Only the command name
            filtered_argv.append(".")

        # Replace sys.argv temporarily
        sys.argv = filtered_argv

        try:
            # Call main directly
            main()
        finally:
            # Restore original argv
            sys.argv = original_argv
    else:
        print(
            "\n💡 No Arduino content found. Create some .ino files and run 'fastled' to compile!"
        )
