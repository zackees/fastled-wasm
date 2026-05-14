"""Examples installation manager using FastLED's built-in --project-init command."""

from fastled._rust_cli import invoke_rust_fastled_cli


def install_fastled_examples_via_project_init(
    force: bool = False, no_interactive: bool = False
) -> bool:
    """
    Install FastLED examples using built-in --project-init command.

    Args:
        force: If True, install without prompting
        no_interactive: If True, skip prompting and return False

    Returns:
        True if installation successful, False otherwise
    """
    if not force:
        if no_interactive:
            print("No existing Arduino content found.")
            print("    In non-interactive mode, skipping examples installation.")
            print("    Run 'fastled --project-init' manually to install examples.")
            return False

        print("No existing Arduino content found.")
        answer = (
            input("Would you like to install FastLED examples? [y/n] ").strip().lower()
        )
        if answer not in ["y", "yes"]:
            print("Skipping FastLED examples installation.")
            return False

    print("Installing FastLED examples using project initialization...")

    try:
        rc = invoke_rust_fastled_cli(["--project-init"])
        if rc != 0:
            print(f"Warning: FastLED project initialization exited with {rc}")
            print("You can manually run: fastled --project-init")
            return False

        print("FastLED project initialized successfully!")
        print("Examples and project structure created")
        print("Quick start: Check for generated .ino files and press F5 to debug")

        return True

    except RuntimeError as e:
        print(f"Warning: Failed to initialize FastLED project: {e}")
        print("    Ensure the native fastled-rs CLI is installed or built.")
        print("Then run: fastled --project-init")
        return False
