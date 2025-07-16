"""
Chrome extension downloader utility for FastLED WASM compiler.

This module provides functionality to download Chrome extensions from the
Chrome Web Store and prepare them for use with Playwright browser.
"""

import os
import re
import shutil
import tempfile
import warnings
import zipfile
from pathlib import Path

import httpx


class ChromeExtensionDownloader:
    """Downloads Chrome extensions from the Chrome Web Store."""

    # Chrome Web Store CRX download URL
    CRX_URL = "https://clients2.google.com/service/update2/crx?response=redirect&prodversion=114.0&acceptformat=crx2,crx3&x=id%3D{extension_id}%26uc"

    # Modern user agent string
    USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36"

    def __init__(self, cache_dir: Path | None = None):
        """Initialize the Chrome extension downloader.

        Args:
            cache_dir: Directory to store downloaded extensions.
                      Defaults to ~/.fastled/chrome-extensions
        """
        if cache_dir is None:
            cache_dir = Path.home() / ".fastled" / "chrome-extensions"

        self.cache_dir = Path(cache_dir)
        self.cache_dir.mkdir(parents=True, exist_ok=True)

        self.headers = {
            "User-Agent": self.USER_AGENT,
            "Referer": "https://chrome.google.com",
        }

    def extract_extension_id(self, url: str) -> str:
        """Extract extension ID from Chrome Web Store URL.

        Args:
            url: Chrome Web Store URL

        Returns:
            Extension ID string

        Raises:
            ValueError: If URL is not a valid Chrome Web Store URL
        """
        # Match new Chrome Web Store URLs (chromewebstore.google.com)
        new_pattern = r"chromewebstore\.google\.com/detail/[^/]+/([a-z]{32})"
        match = re.search(new_pattern, url)

        if match:
            return match.group(1)

        # Match old Chrome Web Store URLs (chrome.google.com/webstore)
        old_pattern = r"chrome\.google\.com/webstore/detail/[^/]+/([a-z]{32})"
        match = re.search(old_pattern, url)

        if match:
            return match.group(1)

        # Try direct extension ID
        if re.match(r"^[a-z]{32}$", url):
            return url

        raise ValueError(f"Invalid Chrome Web Store URL or extension ID: {url}")

    def download_crx(self, extension_id: str) -> bytes:
        """Download CRX file from Chrome Web Store.

        Args:
            extension_id: Chrome extension ID

        Returns:
            CRX file content as bytes

        Raises:
            httpx.RequestError: If download fails
        """
        download_url = self.CRX_URL.format(extension_id=extension_id)

        with httpx.Client(follow_redirects=True) as client:
            response = client.get(download_url, headers=self.headers)
            response.raise_for_status()

            return response.content

    def extract_crx_to_directory(self, crx_content: bytes, extract_dir: Path) -> None:
        """Extract CRX file content to a directory.

        CRX files are essentially ZIP files with a header that needs to be removed.

        Args:
            crx_content: CRX file content as bytes
            extract_dir: Directory to extract the extension to
        """
        # CRX files have a header before the ZIP content
        # We need to find the ZIP header (starts with 'PK')
        zip_start = crx_content.find(b"PK\x03\x04")
        if zip_start == -1:
            zip_start = crx_content.find(b"PK\x05\x06")  # Empty ZIP

        if zip_start == -1:
            raise ValueError("Could not find ZIP header in CRX file")

        # Extract the ZIP portion
        zip_content = crx_content[zip_start:]

        # Create temporary file to extract from
        with tempfile.NamedTemporaryFile(suffix=".zip", delete=False) as temp_zip:
            temp_zip.write(zip_content)
            temp_zip_path = temp_zip.name

        try:
            # Extract the ZIP file
            extract_dir.mkdir(parents=True, exist_ok=True)
            with zipfile.ZipFile(temp_zip_path, "r") as zip_ref:
                zip_ref.extractall(extract_dir)
        finally:
            # Clean up temporary file
            os.unlink(temp_zip_path)

    def get_extension_path(
        self, url_or_id: str, extension_name: str | None = None
    ) -> Path:
        """Download and extract Chrome extension, returning the path to the extracted directory.

        Args:
            url_or_id: Chrome Web Store URL or extension ID
            extension_name: Optional name for the extension directory

        Returns:
            Path to the extracted extension directory
        """
        extension_id = self.extract_extension_id(url_or_id)

        if extension_name is None:
            extension_name = extension_id

        extension_dir = self.cache_dir / extension_name

        # Check if extension is already downloaded and extracted
        if extension_dir.exists() and (extension_dir / "manifest.json").exists():
            print(f"✅ Chrome extension already cached: {extension_dir}")
            return extension_dir

        print(f"🔽 Downloading Chrome extension {extension_id}...")

        try:
            # Download the CRX file
            crx_content = self.download_crx(extension_id)

            # Clean up existing directory if it exists
            if extension_dir.exists():
                shutil.rmtree(extension_dir)

            # Extract the CRX file
            self.extract_crx_to_directory(crx_content, extension_dir)

            # Verify extraction worked
            if not (extension_dir / "manifest.json").exists():
                raise ValueError("Extension extraction failed - no manifest.json found")

            print(f"✅ Chrome extension downloaded and extracted: {extension_dir}")
            return extension_dir

        except Exception as e:
            warnings.warn(f"Failed to download Chrome extension {extension_id}: {e}")
            if extension_dir.exists():
                shutil.rmtree(extension_dir)
            raise


def download_cpp_devtools_extension() -> Path | None:
    """Download the C++ DevTools Support (DWARF) extension.

    Returns:
        Path to the extracted extension directory, or None if download failed
    """
    # C++ DevTools Support (DWARF) extension
    extension_url = "https://chromewebstore.google.com/detail/cc++-devtools-support-dwa/pdcpmagijalfljmkmjngeonclgbbannb"

    try:
        downloader = ChromeExtensionDownloader()
        return downloader.get_extension_path(extension_url, "cpp-devtools-support")
    except Exception as e:
        warnings.warn(f"Failed to download C++ DevTools Support extension: {e}")
        return None


if __name__ == "__main__":
    # Test the downloader with the C++ DevTools Support extension
    extension_path = download_cpp_devtools_extension()
    if extension_path:
        print(f"Extension downloaded to: {extension_path}")
    else:
        print("Failed to download extension")
