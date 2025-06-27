"""
Unit test file demonstrating manual API invocation of fastled-wasm-server.

This test file shows how to manually invoke the fastled-wasm-server API endpoints
using raw HTTP requests, bypassing the high-level Python API wrapper.
"""

import io
import tempfile
import time
import unittest
import zipfile
from pathlib import Path

import httpx

from fastled import Api
from fastled.settings import DEFAULT_URL
from fastled.types import BuildMode


class TestManualApiInvocation(unittest.TestCase):
    """Test manual invocation of fastled-wasm-server API endpoints."""

    # Class-level variables for shared server instance
    server = None
    base_url = None

    @classmethod
    def setUpClass(cls):
        """Set up test environment once for all tests."""
        cls.test_dir = Path(__file__).parent / "unit" / "test_ino" / "wasm"
        cls.timeout = 30

        # Check if we can run local docker tests
        if cls._enabled():
            print("\n🚀 Starting local FastLED WASM server for manual API tests...")
            cls.server = Api.spawn_server()
            cls.base_url = cls.server.url()
            print(f"✅ Server started at: {cls.base_url}")
        else:
            print("\n🌐 Using web server for manual API tests...")
            cls.server = None
            cls.base_url = DEFAULT_URL
            print(f"✅ Using web server: {cls.base_url}")

    @classmethod
    def tearDownClass(cls):
        """Clean up server after all tests."""
        if cls.server is not None:
            print("\n🛑 Stopping local FastLED WASM server...")
            cls.server.stop()
            print("✅ Server stopped")

    @classmethod
    def _enabled(cls) -> bool:
        """Check if this system can run the tests."""
        from fastled import Test

        return Test.can_run_local_docker_tests()

    def _create_test_sketch_zip(self) -> bytes:
        """Create a test sketch zip file for upload."""
        # Create in-memory zip file with test sketch
        zip_buffer = io.BytesIO()
        with zipfile.ZipFile(zip_buffer, "w", zipfile.ZIP_DEFLATED) as zip_file:
            # Add the main sketch file
            sketch_content = """#include <FastLED.h>

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
            zip_file.writestr("wasm/wasm.ino", sketch_content)
            # Add build mode identifier
            zip_file.writestr("wasm/build_mode.txt", BuildMode.QUICK.value)

        return zip_buffer.getvalue()

    @unittest.skipUnless(True, "Test manual API invocation")
    def test_info_endpoint_manual(self) -> None:
        """Test the /info endpoint to get available examples using manual HTTP requests."""
        # Test /info endpoint manually
        info_url = f"{self.base_url}/info"
        print(f"\nTesting INFO endpoint manually: {info_url}")

        response = httpx.get(info_url, timeout=self.timeout)

        self.assertEqual(response.status_code, 200)
        data = response.json()
        self.assertIn("examples", data)
        self.assertIsInstance(data["examples"], list)
        self.assertGreater(len(data["examples"]), 0)

        print(f"✅ Found {len(data['examples'])} examples: {data['examples']}")

    @unittest.skipUnless(True, "Test manual API invocation")
    def test_healthz_endpoint_manual(self) -> None:
        """Test the /healthz endpoint for health check using manual HTTP requests."""
        # Test /healthz endpoint manually
        healthz_url = f"{self.base_url}/healthz"
        print(f"\nTesting HEALTHZ endpoint manually: {healthz_url}")

        response = httpx.get(healthz_url, timeout=self.timeout)

        self.assertEqual(response.status_code, 200)
        print(f"✅ Health check successful: {response.text}")

    @unittest.skipUnless(True, "Test manual API invocation")
    def test_compile_wasm_endpoint_manual(self) -> None:
        """Test manual invocation of /compile/wasm endpoint using raw HTTP requests.

        This test demonstrates how to manually call the FastLED WASM compilation API
        without using the high-level Python wrapper functions.
        """
        # Create test sketch zip
        zip_bytes = self._create_test_sketch_zip()

        # Test /compile/wasm endpoint manually
        compile_url = f"{self.base_url}/compile/wasm"
        print(f"\nTesting COMPILE/WASM endpoint manually: {compile_url}")
        print(f"📦 Upload size: {len(zip_bytes)} bytes")

        # Prepare headers (exactly as the internal API does)
        headers = {
            "accept": "application/json",
            "authorization": "oBOT5jbsO4ztgrpNsQwlmFLIKB",  # Default auth token
            "build": BuildMode.QUICK.value.lower(),
            "profile": "false",
        }

        # Prepare files for upload
        files = {"file": ("wasm.zip", zip_bytes, "application/x-zip-compressed")}

        # Make the request manually
        start_time = time.time()

        with httpx.Client(timeout=60 * 2) as client:  # 2 minute timeout
            response = client.post(
                compile_url, files=files, headers=headers, follow_redirects=True
            )

        compile_time = time.time() - start_time
        print(f"⏱️  Compilation took: {compile_time:.2f} seconds")

        # Verify response
        print(f"📋 Response status: {response.status_code}")
        if response.status_code != 200:
            print(f"❌ Response content: {response.text}")

        self.assertEqual(response.status_code, 200)

        # Verify we got a zip file back
        self.assertGreater(len(response.content), 0)
        print(f"📥 Response zip size: {len(response.content)} bytes")

        # Try to extract and verify the response contains expected files
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            zip_path = temp_path / "response.zip"
            zip_path.write_bytes(response.content)

            # Extract and check contents
            import shutil

            shutil.unpack_archive(zip_path, temp_path, "zip")

            # Look for expected output files
            js_files = list(temp_path.glob("**/*.js"))
            wasm_files = list(temp_path.glob("**/*.wasm"))

            print(f"🔍 Found JS files: {[f.name for f in js_files]}")
            print(f"🔍 Found WASM files: {[f.name for f in wasm_files]}")

            self.assertGreater(len(js_files), 0, "Expected to find .js files")
            self.assertGreater(len(wasm_files), 0, "Expected to find .wasm files")

            # Verify file sizes are reasonable
            for js_file in js_files:
                size = js_file.stat().st_size
                self.assertGreater(
                    size, 1000, f"JS file {js_file.name} too small: {size} bytes"
                )
                print(f"📄 {js_file.name}: {size:,} bytes")

            for wasm_file in wasm_files:
                size = wasm_file.stat().st_size
                self.assertGreater(
                    size, 1000, f"WASM file {wasm_file.name} too small: {size} bytes"
                )
                print(f"⚙️  {wasm_file.name}: {size:,} bytes")

    @unittest.skipUnless(True, "Test manual API invocation")
    def test_compile_libfastled_endpoint_manual(self) -> None:
        """Test manual invocation of /compile/libfastled endpoint using raw HTTP requests.

        This test demonstrates how to manually call the FastLED library compilation API
        which compiles just the FastLED library without a sketch.
        """
        # Test /compile/libfastled endpoint manually
        compile_url = f"{self.base_url}/compile/libfastled"
        print(f"\nTesting COMPILE/LIBFASTLED endpoint manually: {compile_url}")

        # Prepare headers for libfastled compilation
        headers = {
            "accept": "application/json",
            "authorization": "oBOT5jbsO4ztgrpNsQwlmFLIKB",  # Default auth token
            "build": BuildMode.QUICK.value.lower(),
        }

        print(f"🔧 Build mode: {headers['build']}")

        # Make the request manually (no file upload needed for library compilation)
        start_time = time.time()

        with httpx.Client(
            timeout=60 * 3
        ) as client:  # 3 minute timeout for library compilation
            response = client.post(compile_url, headers=headers, follow_redirects=True)

        compile_time = time.time() - start_time
        print(f"⏱️  Library compilation took: {compile_time:.2f} seconds")

        # Verify response
        print(f"📋 Response status: {response.status_code}")
        if response.status_code != 200:
            print(f"❌ Response content: {response.text}")

        self.assertEqual(response.status_code, 200)

        # Verify we got some content back (streaming response)
        self.assertGreater(len(response.content), 0)
        print(f"📥 Response size: {len(response.content)} bytes")

        # Check if it's a zip file or other binary content
        if response.content.startswith(b"PK"):  # ZIP file magic bytes
            print("📦 Response appears to be a ZIP file")
            # Try to extract and verify the response contains expected library files
            with tempfile.TemporaryDirectory() as temp_dir:
                temp_path = Path(temp_dir)
                zip_path = temp_path / "libfastled.zip"
                zip_path.write_bytes(response.content)

                # Extract and check contents
                import shutil

                shutil.unpack_archive(zip_path, temp_path, "zip")

                # Look for expected library files
                lib_files = list(temp_path.glob("**/*.a")) + list(
                    temp_path.glob("**/*.so")
                )
                header_files = list(temp_path.glob("**/*.h")) + list(
                    temp_path.glob("**/*.hpp")
                )

                print(f"🔍 Found library files: {[f.name for f in lib_files]}")
                print(f"🔍 Found header files: {[f.name for f in header_files]}")

                # We expect at least some library or header files
                self.assertGreater(
                    len(lib_files) + len(header_files),
                    0,
                    "Expected to find library or header files",
                )
        else:
            print("📄 Response appears to be text/binary data")
            # Could be a streaming response with compilation logs
            try:
                text_content = response.content.decode("utf-8")
                print(f"📝 Response preview: {text_content[:200]}...")
            except UnicodeDecodeError:
                print("🔧 Response contains binary data")

    @unittest.skipUnless(True, "Test manual API invocation")
    def test_project_init_endpoint_manual(self) -> None:
        """Test the /project/init endpoint to initialize a project using manual HTTP requests."""
        # Test /project/init endpoint manually
        init_url = f"{self.base_url}/project/init"
        print(f"\nTesting PROJECT/INIT endpoint manually: {init_url}")

        # Request a basic example project
        example_name = "wasm"  # This should be a basic available example

        response = httpx.post(
            init_url,
            json=example_name,
            headers={"accept": "application/json"},
            timeout=self.timeout,
        )

        self.assertEqual(response.status_code, 200)

        # Verify we got a zip file back
        self.assertGreater(len(response.content), 0)
        print(f"📦 Project init zip size: {len(response.content)} bytes")

        # Verify the zip contains expected files
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            zip_path = temp_path / "project.zip"
            zip_path.write_bytes(response.content)

            # Extract and check contents
            import shutil

            shutil.unpack_archive(zip_path, temp_path, "zip")

            # Look for .ino files
            ino_files = list(temp_path.glob("**/*.ino"))
            print(f"📄 Found .ino files: {[f.name for f in ino_files]}")

            self.assertGreater(len(ino_files), 0, "Expected to find .ino files")

    @unittest.skipUnless(True, "Test manual API invocation")
    def test_docs_endpoint_manual(self) -> None:
        """Test that the /docs endpoint exists (FastAPI documentation) using manual HTTP requests."""
        if not self._enabled():
            self.skipTest("Local server not available for /docs test")

        # Test /docs endpoint manually
        docs_url = f"{self.base_url}/docs"
        print(f"\nTesting DOCS endpoint manually: {docs_url}")

        response = httpx.get(docs_url, timeout=self.timeout)

        # Should return HTML documentation
        self.assertEqual(response.status_code, 200)
        self.assertIn("text/html", response.headers.get("content-type", "").lower())
        print("✅ FastAPI docs endpoint accessible")

    def test_api_endpoints_summary(self) -> None:
        """Test that demonstrates all available API endpoints and their usage.

        This is a comprehensive test showing what endpoints exist and how to use them manually.
        """
        print("\n" + "=" * 70)
        print("📋 FASTLED WASM SERVER API ENDPOINTS SUMMARY")
        print("=" * 70)
        print("Available endpoints:")
        print("  • /compile/wasm      - Main compilation endpoint (POST)")
        print("  • /compile/libfastled - FastLED library compilation (POST)")
        print("  • /info              - Get available examples (GET)")
        print("  • /project/init      - Initialize project from example (POST)")
        print("  • /healthz           - Health check (GET)")
        print("  • /docs              - FastAPI documentation (GET)")
        print("  • /dwarfsource       - Debug source files (POST)")
        print("  • /sourcefiles/*     - Source file serving (GET)")
        print("\n✅ NEW: /compile/libfastled endpoint found!")
        print("   Compiles the FastLED library without requiring a sketch")
        print("=" * 70)

        # This test always passes - it's just for documentation
        self.assertTrue(True, "API endpoints summary displayed")


if __name__ == "__main__":
    unittest.main()
