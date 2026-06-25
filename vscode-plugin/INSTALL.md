# Installation Instructions

## Prerequisites

Before installing the FastLED WASM VS Code Extension, make sure you have:

1. **VS Code** - [Download VS Code](https://code.visualstudio.com/)
2. **FastLED WASM Compiler** - Install via:
   ```bash
   pip install fastled
   ```
3. **Docker** (optional but recommended for local compilation)

## Installation Methods

### Method 1: From VS Code Marketplace (Coming Soon)

1. Open VS Code
2. Go to Extensions (Ctrl+Shift+X / Cmd+Shift+X)
3. Search for "FastLED WASM"
4. Click "Install"

### Method 2: Manual Installation (Development)

1. **Clone the repository:**
   ```bash
   git clone https://github.com/zackees/fastled-wasm.git
   cd fastled-wasm/vscode-plugin
   ```

2. **Install dependencies:**
   ```bash
   npm install
   ```

3. **Compile the extension:**
   ```bash
   npm run compile
   ```

4. **Run in development mode:**
   - Open the `vscode-plugin` folder in VS Code
   - Press `F5` to launch Extension Development Host
   - Test the extension in the new window

### Method 3: Package and Install

1. **Install vsce (VS Code Extension Manager):**
   ```bash
   npm install -g vsce
   ```

2. **Navigate to the plugin directory:**
   ```bash
   cd vscode-plugin
   ```

3. **Package the extension:**
   ```bash
   vsce package
   ```

4. **Install the packaged extension:**
   ```bash
   code --install-extension fastled-wasm-1.0.0.vsix
   ```

## Quick Setup Script

For Linux/macOS users, you can use the setup script:

```bash
cd vscode-plugin
chmod +x setup.sh
./setup.sh
```

## Verification

After installation, verify the extension is working:

1. Open VS Code
2. Create a new folder for your FastLED project
3. Press `Ctrl+Shift+P` (or `Cmd+Shift+P` on Mac)
4. Type "FastLED" - you should see the FastLED commands
5. Try "FastLED: Initialize New Project" to create a sample project

## Troubleshooting

### Extension not showing up
- Make sure VS Code is restarted after installation
- Check Extensions view to see if the extension is enabled

### FastLED commands not working
- Verify FastLED WASM compiler is installed: `fastled --version`
- Check the Output panel (View > Output > FastLED WASM) for error messages

### Docker issues
- Make sure Docker is installed and running
- Try using web compiler mode if Docker fails

## Next Steps

Once installed, check out:
- [README.md](README.md) for usage instructions
- [CHANGELOG.md](CHANGELOG.md) for version history
- The FastLED examples to get started 