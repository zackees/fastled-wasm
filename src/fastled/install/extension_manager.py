"""Auto Debug extension installation manager."""

import shutil
import subprocess
import tempfile
from pathlib import Path
from urllib.request import urlretrieve


def download_auto_debug_extension() -> Path | None:
    """
    Download the Auto Debug extension .vsix file from GitHub.

    Returns:
        Path to downloaded .vsix file, or None if download fails
    """
    # URL for the Auto Debug extension
    extension_url = "https://github.com/zackees/vscode-auto-debug/releases/latest/download/auto-debug.vsix"

    try:
        # Create temporary directory
        temp_dir = Path(tempfile.mkdtemp())
        vsix_path = temp_dir / "auto-debug.vsix"

        print("📥 Downloading Auto Debug extension...")

        # Download the file
        urlretrieve(extension_url, vsix_path)

        if vsix_path.exists() and vsix_path.stat().st_size > 0:
            print("✅ Extension downloaded successfully")
            return vsix_path
        else:
            print("❌ Failed to download extension")
            return None

    except Exception as e:
        print(f"❌ Error downloading extension: {e}")
        return None


def install_vscode_extensions(extension_path: Path) -> bool:
    """
    Install extension in VSCode or Cursor.

    Args:
        extension_path: Path to .vsix file

    Returns:
        True if installation successful, False otherwise
    """
    # Try VSCode first
    if shutil.which("code"):
        ide_command = "code"
        ide_name = "VSCode"
    elif shutil.which("cursor"):
        ide_command = "cursor"
        ide_name = "Cursor"
    else:
        print("❌ No supported IDE found (VSCode or Cursor)")
        return False

    try:
        print(f"📦 Installing extension in {ide_name}...")

        # Install the extension
        subprocess.run(
            [ide_command, "--install-extension", str(extension_path)],
            capture_output=True,
            text=True,
            check=True,
        )

        print(f"✅ Auto Debug extension installed in {ide_name}")
        return True

    except subprocess.CalledProcessError as e:
        print(f"❌ Failed to install extension: {e}")
        if e.stderr:
            print(f"Error details: {e.stderr}")
        return False
    finally:
        # Clean up temporary file
        if extension_path.exists():
            try:
                extension_path.unlink()
                extension_path.parent.rmdir()
            except Exception:
                pass


def install_auto_debug_extension(dry_run: bool = False) -> bool:
    """
    Main function to download and install Auto Debug extension.

    Args:
        dry_run: If True, simulate installation without actually installing

    Returns:
        True if installation successful, False otherwise
    """
    if dry_run:
        print("[DRY-RUN]: Would download and install Auto Debug extension")
        print("[DRY-RUN]: NO PLUGIN INSTALLED")
        return True

    # Download extension
    vsix_path = download_auto_debug_extension()
    if not vsix_path:
        return False

    # Install extension
    return install_vscode_extensions(vsix_path)
