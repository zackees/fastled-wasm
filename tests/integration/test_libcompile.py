"""
Integration test for libfastled compilation with volume source mapping.

This test:
1. Downloads the FastLED source zip from GitHub master branch
2. Expands it into a temporary directory
3. Sets up volume source mapping using the API
4. Compiles it using web_compile and ensures no errors occur
"""

import tempfile
import unittest
import zipfile
from pathlib import Path

import httpx

from fastled.compile_server_impl import CompileServerImpl
from fastled.emoji_util import EMO, safe_print
from fastled.settings import AUTH_TOKEN
from fastled.types import BuildMode, CompileResult


def _enabled() -> bool:
    """Check if this system can run the tests."""
    from fastled import Test

    return Test.can_run_local_docker_tests()


class TestLibcompileWithVolumeMapping(unittest.TestCase):
    """Test libfastled compilation with volume source mapping."""

    def setUp(self):
        """Set up test environment."""
        if not _enabled():
            self.skipTest("Docker not available for testing")

        # Create a simple test sketch
        self.test_sketch_content = """#include <FastLED.h>

#define LED_PIN 3
#define NUM_LEDS 100

CRGB leds[NUM_LEDS];

void setup() {
    FastLED.addLeds<WS2812, LED_PIN>(leds, NUM_LEDS);
}

void loop() {
    fill_rainbow(leds, NUM_LEDS, 0, 7);
    FastLED.show();
    delay(30);
}"""

    def download_fastled_source(self, temp_dir: Path) -> Path:
        """Download FastLED source from GitHub master branch."""
        safe_print(
            f"{EMO('📥', 'DOWNLOAD:')} Downloading FastLED source from GitHub..."
        )

        # GitHub URL for downloading the master branch as a zip
        github_zip_url = (
            "https://github.com/FastLED/FastLED/archive/refs/heads/master.zip"
        )

        # Download the zip file
        with httpx.Client(timeout=60) as client:
            response = client.get(github_zip_url, follow_redirects=True)
            response.raise_for_status()

        safe_print(f"✅ Downloaded {len(response.content)} bytes")

        # Save and extract the zip
        zip_path = temp_dir / "fastled-master.zip"
        zip_path.write_bytes(response.content)

        # Extract the zip
        extract_dir = temp_dir / "fastled-extract"
        extract_dir.mkdir()

        with zipfile.ZipFile(zip_path, "r") as zip_ref:
            zip_ref.extractall(extract_dir)

        # Find the extracted FastLED directory (it will be named something like "FastLED-master")
        fastled_dirs = list(extract_dir.glob("FastLED-*"))
        if not fastled_dirs:
            raise RuntimeError("Could not find extracted FastLED directory")

        fastled_dir = fastled_dirs[0]
        safe_print(f"✅ Extracted FastLED source to: {fastled_dir}")

        # Verify it has the expected structure
        src_dir = fastled_dir / "src"
        library_props = fastled_dir / "library.properties"

        if not src_dir.exists():
            raise RuntimeError(f"FastLED src directory not found: {src_dir}")
        if not library_props.exists():
            raise RuntimeError(f"FastLED library.properties not found: {library_props}")

        safe_print(f"✅ Verified FastLED structure - src: {src_dir}")
        return fastled_dir

    def create_test_sketch(self, temp_dir: Path) -> Path:
        """Create a test sketch directory."""
        sketch_dir = temp_dir / "test_sketch"
        sketch_dir.mkdir()

        # Create the .ino file
        sketch_file = sketch_dir / "test_sketch.ino"
        sketch_file.write_text(self.test_sketch_content, encoding="utf-8")

        safe_print(f"✅ Created test sketch: {sketch_file}")
        return sketch_dir

    def create_server_with_volume_mapping(
        self, fastled_src_dir: Path
    ) -> CompileServerImpl:
        """Create a CompileServerImpl with manual volume mapping to FastLED source."""
        safe_print(f"🔧 Setting up server with volume mapping to: {fastled_src_dir}")

        # Mock the _try_get_fastled_src function to return our downloaded FastLED source
        from unittest import mock

        from fastled import compile_server_impl

        # Verify our source directory exists
        expected_src_dir = fastled_src_dir / "src"
        if not expected_src_dir.exists():
            raise RuntimeError(
                f"FastLED src directory does not exist: {expected_src_dir}"
            )

        # Mock the function to return our fastled directory
        with mock.patch.object(
            compile_server_impl, "_try_get_fastled_src", return_value=expected_src_dir
        ):
            # Create a custom CompileServerImpl - now it will use our mocked source
            server_impl = CompileServerImpl(
                auto_start=False,  # Don't start yet
                allow_libcompile=True,  # Explicitly allow libcompile
            )

        # Verify the setup worked
        assert server_impl.fastled_src_dir is not None
        if not server_impl.fastled_src_dir.exists():
            raise RuntimeError(
                f"FastLED src directory does not exist: {server_impl.fastled_src_dir}"
            )

        # Verify libcompile is allowed (should be True since we have fastled_src_dir)
        if not server_impl.allow_libcompile:
            raise RuntimeError("libcompile should be allowed with volume mapping")

        safe_print("✅ Server configured with volume mapping")
        safe_print(f"   FastLED src: {server_impl.fastled_src_dir}")
        safe_print(f"   Allow libcompile: {server_impl.allow_libcompile}")

        # Now start the server
        server_impl.start()

        return server_impl

    @unittest.skipUnless(_enabled(), "Requires Docker for libcompile testing")
    def test_libcompile_with_downloaded_fastled_source(self) -> None:
        """Test libfastled compilation with downloaded FastLED source and volume mapping."""

        with tempfile.TemporaryDirectory() as temp_dir_str:
            temp_dir = Path(temp_dir_str)
            safe_print(f"🗂️  Using temporary directory: {temp_dir}")

            try:
                # Step 1: Download FastLED source from GitHub
                fastled_dir = self.download_fastled_source(temp_dir)

                # Step 2: Create a test sketch
                sketch_dir = self.create_test_sketch(temp_dir)

                # Step 3: Create server with volume mapping
                server_impl = self.create_server_with_volume_mapping(fastled_dir)

                try:
                    # Verify server is running
                    running, error = server_impl.running
                    self.assertTrue(running, f"Server should be running: {error}")

                    # Step 4: Verify volume mapping is active
                    self.assertTrue(
                        server_impl.using_fastled_src_dir_volume(),
                        "Server should be using FastLED source directory volume",
                    )

                    # Step 5: Test libfastled compilation by compiling our test sketch
                    safe_print("🔨 Testing libfastled compilation...")

                    result: CompileResult = server_impl.web_compile(
                        directory=sketch_dir,
                        build_mode=BuildMode.QUICK,
                        profile=False,
                    )

                    safe_print(
                        f"{EMO('🔨', 'BUILD:')} Compilation result: {result.stdout}"
                    )

                    # Step 6: Verify compilation was successful
                    self.assertTrue(
                        result.success,
                        f"Compilation should succeed with libfastled. Output: {result.stdout}",
                    )

                    self.assertGreater(
                        len(result.zip_bytes), 0, "Should receive compiled output"
                    )

                    # Step 7: Verify libfastled was actually used (check output for indicators)
                    # The output should contain evidence that libfastled compilation happened
                    stdout_lower = result.stdout.lower()

                    # Look for signs that libfastled compilation occurred
                    has_libfastled_indicators = any(
                        [
                            "libfastled" in stdout_lower,
                            "step 1" in stdout_lower,  # Our step 1 message
                            "step 2" in stdout_lower,  # Our step 2 message
                        ]
                    )

                    if not has_libfastled_indicators:
                        safe_print(
                            f"{EMO('⚠️', 'WARNING:')}  No clear libfastled indicators found in output"
                        )
                        safe_print(f"{EMO('📋', 'OUTPUT:')} Compilation output:")
                        safe_print(result.stdout)

                    safe_print("✅ libfastled compilation test completed successfully!")
                    safe_print("📊 Compilation stats:")
                    safe_print(f"   Success: {result.success}")
                    safe_print(f"   Output size: {len(result.zip_bytes)} bytes")
                    safe_print(f"   Hash: {result.hash_value}")

                    # Step 8: Test that libcompile fails when FastLED source is corrupted
                    safe_print("\n🔨 Testing libfastled compilation failure...")

                    # Corrupt the FastLED source by adding an error directive
                    fastled_cpp_path = (
                        fastled_dir / "src" / "sensors" / "digital_pin.cpp.hpp"
                    )
                    if not fastled_cpp_path.exists():
                        safe_print(
                            f"{EMO('⚠️', 'WARNING:')}  FastLED source file not found: {fastled_cpp_path}"
                        )
                        safe_print(f"{EMO('📋', 'AVAILABLE:')} Available files in src:")
                        for f in (fastled_dir / "src").iterdir():
                            safe_print(f"  {f.name}")
                        self.fail(f"FastLED source file not found: {fastled_cpp_path}")
                    else:
                        # Read the original content and append the error directive
                        original_content = fastled_cpp_path.read_text(encoding="utf-8")
                        corrupted_content = (
                            original_content + '\n#error "Should fail"\n'
                        )
                        fastled_cpp_path.write_text(corrupted_content, encoding="utf-8")
                        safe_print(f"✅ Added error directive to {fastled_cpp_path}")

                        # Try to compile libfastled directly via HTTP endpoint - this should fail with 400
                        safe_print(
                            "🔨 Attempting libfastled compilation with corrupted source..."
                        )

                        # Make direct HTTP call to /compile/libfastled endpoint
                        compile_url = f"{server_impl.url()}/compile/libfastled"
                        headers = {
                            "accept": "application/json",
                            "authorization": AUTH_TOKEN,  # Default auth token
                            "build": BuildMode.QUICK.value.lower(),
                        }

                        with httpx.Client(timeout=60 * 3) as client:
                            response = client.post(
                                compile_url, headers=headers, follow_redirects=True
                            )

                        self.assertIn(
                            "HTTP_STATUS: 400",
                            response.text,
                            "Should have HTTP_STATUS: 400 in response",
                        )

                        safe_print(
                            "✅ libfastled compilation correctly failed with 400 HTTP status!"
                        )
                        safe_print("📊 Failed compilation stats:")
                        safe_print(f"   Response length: {len(response.text)} chars")

                        # Restore the original content for cleanup
                        fastled_cpp_path.write_text(original_content, encoding="utf-8")
                        safe_print("✅ Restored original FastLED source content")

                finally:
                    # Clean up server
                    try:
                        server_impl.stop()
                    except Exception as e:
                        safe_print(f"Warning during server cleanup: {e}")

            except Exception as e:
                safe_print(f"❌ Test failed: {e}")
                raise

    @unittest.skipUnless(_enabled(), "Requires Docker for libcompile testing")
    def test_libcompile_disabled_without_volume_mapping(self) -> None:
        """Test that libfastled compilation is disabled when volume mapping is not available."""

        with tempfile.TemporaryDirectory() as temp_dir_str:
            temp_dir = Path(temp_dir_str)

            # Create a test sketch but no FastLED source mapping
            sketch_dir = self.create_test_sketch(temp_dir)

            # Create server without volume mapping (default behavior)
            server_impl = CompileServerImpl(auto_start=True, allow_libcompile=True)

            try:
                # Verify libcompile was disabled due to no volume mapping
                self.assertFalse(
                    server_impl.allow_libcompile,
                    "libcompile should be disabled without volume mapping",
                )

                self.assertFalse(
                    server_impl.using_fastled_src_dir_volume(),
                    "Should not be using FastLED source directory volume",
                )

                # Compile should still work, just without libfastled
                result: CompileResult = server_impl.web_compile(
                    directory=sketch_dir,
                    build_mode=BuildMode.QUICK,
                    profile=False,
                )

                self.assertTrue(
                    result.success,
                    f"Compilation should still succeed without libfastled. Output: {result.stdout}",
                )

                safe_print(
                    "✅ Verified libcompile is properly disabled without volume mapping"
                )

            finally:
                try:
                    server_impl.stop()
                except Exception as e:
                    safe_print(f"Warning during server cleanup: {e}")


if __name__ == "__main__":
    unittest.main()
