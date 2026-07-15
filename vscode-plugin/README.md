# FastLED WASM VS Code Extension

A VS Code extension for the FastLED WASM compiler that allows you to compile and run FastLED sketches directly in your browser.

## Bundled clangd

Target-specific VSIX packages contain a verified native clangd server for
Windows x64/ARM64, Linux x64/ARM64, and macOS x64/ARM64. The universal VSIX is
intentionally offline-only: it contains no server and reports a structured
`universal-package` result to the extension API. No package downloads clangd,
searches `PATH`, or modifies dynamic-library environment variables at runtime.

To reproduce a package locally (Node 22 and `uv` required):

```sh
cd vscode-plugin
npm ci
python scripts/package_extension.py --target win32-x64 --out dist
python scripts/package_extension.py --target universal --out dist
```

Use **FastLED: Show Bundled clangd Diagnostics** to see the resolved target,
version and path, or a non-sensitive structured failure reason. Delete
`resources/clangd` (or run `python scripts/ingest_clangd.py --clean --output
resources/clangd`) if an interrupted local package needs to be reset. The
artifact lock is intentionally pinned to `clang-tool-chain-bins` 0.4.6;
updating clangd requires changing that reviewed lock rather than resolving a
latest version.

## IntelliSense and navigation

FastLED configures one C/C++ language engine for the whole VS Code window.
The extension pack installs both the LLVM clangd and Microsoft C/C++
extensions, but it deliberately never leaves both language services active
for the same sketch:

- `auto` (default) uses the native clangd bundled in a platform VSIX when the
  clangd VS Code extension is available; otherwise it uses Microsoft C/C++.
- `clangd`, `cpptools`, and `off` are explicit choices in the
  `fastled.intelliSenseEngine` setting.
- **FastLED: Refresh IntelliSense Configuration** regenerates the
  `compile_commands.json`, `.clangd`, `.vscode/settings.json`, and
  `.vscode/c_cpp_properties.json` files for every FastLED sketch folder.

The generated settings associate `.ino` files with C++, while FastLED keeps a
preprocessed live snapshot and prototype prelude current for unsaved Arduino
tabs. After opening a Blink sketch, use Go to Definition on `CRGB`, `FastLED`,
or `FastLED.addLeds`; the result should open the materialized FastLED headers.
Use **View: Output** → **FastLED WASM** to inspect engine selection and setup
failures.

## Features

- **One-click compilation**: Compile and run FastLED sketches with a single command
- **Multiple build modes**: Quick, Debug, and Release compilation modes
- **Project initialization**: Initialize new FastLED projects with example sketches
- **Server management**: Start and stop the FastLED compiler server
- **Syntax highlighting**: Full syntax highlighting for Arduino (.ino) files with FastLED-specific keywords
- **Code snippets**: Common FastLED patterns and boilerplate code
- **Browser integration**: Automatically open compiled results in your browser
- **File watching**: Automatic recompilation on file changes (when supported)

## Requirements

- [FastLED WASM compiler](https://github.com/zackees/fastled-wasm) must be installed and available in your PATH
- Docker (recommended for local compilation) or internet connection for web compiler

## Installation

### From VS Code Marketplace (Coming Soon)
1. Open VS Code
2. Go to Extensions (Ctrl+Shift+X)
3. Search for "FastLED WASM"
4. Click Install

### Manual Installation
1. Clone this repository
2. Navigate to the `vscode-plugin` directory
3. Run `npm install`
4. Run `npm run compile`
5. Press F5 to open a new Extension Development Host window

## Commands

Access all commands via the Command Palette (Ctrl+Shift+P) or through the context menu:

### Compilation Commands
- **FastLED: Compile & Run** - Compile the current sketch and open in browser
- **FastLED: Compile & Run (Quick Mode)** - Fast compilation with minimal optimizations
- **FastLED: Compile & Run (Web Compiler)** - Use the online web compiler
- **FastLED: Just Compile (No Browser)** - Compile without opening browser

### Project Management
- **FastLED: Initialize New Project** - Create a new FastLED project with example code
- **FastLED: Open FastLED Output in Browser** - Open the last compiled output

### Server Management  
- **FastLED: Start Compiler Server** - Start a local FastLED compiler server
- **FastLED: Stop Compiler Server** - Stop the running server

### Maintenance
- **FastLED: Update Compiler** - Update the FastLED compiler to the latest version
- **FastLED: Purge Docker Containers** - Clean up FastLED Docker containers and images

## Configuration

Configure the extension through VS Code Settings (File > Preferences > Settings, search for "FastLED"):

- **Default Compile Mode**: Choose between quick, debug, or release mode (default: quick)
- **Auto Open Browser**: Automatically open browser after compilation (default: true)
- **Use Web Compiler**: Use web compiler by default instead of local Docker (default: false)
- **Web Compiler URL**: URL for the web compiler (default: https://fastled.onrender.com)
- **Watch Files**: Enable file watching for automatic recompilation (default: true)

## Usage

### Getting Started
1. Open a folder containing FastLED sketches (.ino files)
2. The extension will automatically detect FastLED projects
3. Use **FastLED: Compile & Run** to compile and view your sketch

### Creating a New Project
1. Open an empty folder in VS Code
2. Run **FastLED: Initialize New Project**
3. Select an example from the list
4. The project will be initialized with the selected example

### Compiling Sketches
1. Open a FastLED sketch (.ino file)
2. Right-click in the editor and select **FastLED: Compile & Run**
3. Or use the Command Palette (Ctrl+Shift+P) and search for FastLED commands
4. The compiled output will open in your default browser

## Syntax Highlighting

The extension provides comprehensive syntax highlighting for:
- Standard Arduino functions (setup, loop, pinMode, digitalWrite, etc.)
- FastLED types (CRGB, CHSV, CRGBPalette16, etc.)
- FastLED functions (addLeds, show, setBrightness, fill_solid, etc.)
- FastLED constants (WS2812B, APA102, color names, etc.)

## Code Snippets

Type these prefixes and press Tab to expand:

- `fastled-setup` - Basic FastLED setup with common configuration
- `fastled-clock` - FastLED setup with clock pin for APA102/DotStar LEDs
- `fill-solid` - Fill all LEDs with solid color
- `fill-rainbow` - Fill LEDs with rainbow colors
- `every-n-ms` - Execute code every N milliseconds
- `fade-black` - Fade all LEDs toward black
- `beatsin8` - 8-bit sine wave that cycles with a given BPM
- `random-color` - Generate a random RGB color
- `hsv-color` - Create HSV color
- `palette-color` - Get color from palette
- `fire-effect` - Simple fire effect
- `sparkle-effect` - Sparkle effect with fade
- `wave-effect` - Simple wave effect

## Troubleshooting

### FastLED command not found
Make sure the FastLED WASM compiler is installed and available in your PATH:
```bash
npm install -g fastled
# or
pip install fastled
```

### Docker issues
If you're having issues with local compilation:
1. Make sure Docker is installed and running
2. Try using the web compiler with **FastLED: Compile & Run (Web Compiler)**
3. Use **FastLED: Update Compiler** to get the latest version

### Compilation errors
1. Check the Output panel (View > Output, select "FastLED WASM")
2. Ensure your sketch directory contains a valid .ino file
3. Try **FastLED: Update Compiler** if you're using an older version

## Contributing

This extension is part of the [FastLED WASM project](https://github.com/zackees/fastled-wasm). 

To contribute:
1. Fork the repository
2. Make your changes in the `vscode-plugin` directory
3. Test your changes
4. Submit a pull request

## License

This project is licensed under the MIT License - see the [LICENSE](../LICENSE) file for details.

## Links

- [FastLED WASM Compiler](https://github.com/zackees/fastled-wasm)
- [FastLED Library](https://github.com/FastLED/FastLED)
- [Online Demo](https://zackees.github.io/fastled-wasm/)
