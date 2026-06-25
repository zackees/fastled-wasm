# FastLED WASM VS Code Extension - Quick Setup

This directory contains the VS Code extension for the FastLED WASM compiler. This extension allows developers to compile and run FastLED sketches directly in VS Code with one-click compilation, syntax highlighting, and project management features.

## Prerequisites

Before you begin, ensure you have:

- **Node.js** (v16 or later) - [Download Node.js](https://nodejs.org/)
- **VS Code** - [Download VS Code](https://code.visualstudio.com/)
- **FastLED WASM Compiler** - Install via: `pip install fastled`

## Quick Start

### 1. Setup the Extension

```bash
# Navigate to the plugin directory
cd vscode-plugin

# Install dependencies
npm install

# Compile TypeScript
npm run compile
```

### 2. Run in Development Mode

1. Open this `vscode-plugin` folder in VS Code
2. Press `F5` to launch a new Extension Development Host window
3. In the new window, you can test all FastLED commands

### 3. Test the Extension

In the Extension Development Host window:

1. Open the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`)
2. Type "FastLED" to see available commands
3. Try `FastLED: Initialize New Project` to create a sample project
4. Use `FastLED: Compile & Run` to compile a sketch

## Available Commands

- **FastLED: Compile & Run** - Compile current sketch and open in browser
- **FastLED: Compile & Run (Quick Mode)** - Fast compilation 
- **FastLED: Compile & Run (Web Compiler)** - Use online compiler
- **FastLED: Initialize New Project** - Create new project with examples
- **FastLED: Start/Stop Compiler Server** - Manage local compiler server
- **FastLED: Update Compiler** - Update FastLED WASM compiler

## Package for Distribution

To create a distributable extension package:

```bash
# Install VS Code Extension Manager
npm install -g vsce

# Package the extension
vsce package

# This creates fastled-wasm-1.0.0.vsix
```

## Install Packaged Extension

```bash
# Install the extension in VS Code
code --install-extension fastled-wasm-1.0.0.vsix
```

## Development Workflow

1. **Make changes** to TypeScript files in `src/`
2. **Compile** with `npm run compile` or `npm run watch`
3. **Reload** the Extension Development Host window (`Ctrl+R` / `Cmd+R`)
4. **Test** your changes

## File Structure

```
vscode-plugin/
├── package.json              # Extension manifest
├── src/extension.ts          # Main extension code
├── syntaxes/                 # Syntax highlighting
├── snippets/                 # Code snippets
├── language-configuration.json
└── README.md                 # Full documentation
```

## Troubleshooting

### Extension not loading
- Ensure all dependencies are installed: `npm install`
- Compile TypeScript: `npm run compile`
- Check VS Code Developer Tools (Help > Toggle Developer Tools)

### FastLED commands not working
- Verify FastLED is installed: `fastled --version`
- Check Output panel: View > Output > Select "FastLED WASM"

### TypeScript compilation errors
- Run `npm run compile` to see detailed errors
- Ensure TypeScript version matches package.json

## Next Steps

- See [README.md](README.md) for full documentation
- Check [INSTALL.md](INSTALL.md) for detailed installation options
- Review [CHANGELOG.md](CHANGELOG.md) for version history

## Quick Commands Reference

```bash
# Development setup
npm install && npm run compile

# Run extension
# Press F5 in VS Code

# Package extension
npm install -g vsce && vsce package

# Install extension
code --install-extension fastled-wasm-1.0.0.vsix
```

---

**Ready to develop FastLED sketches in VS Code!** 🚀 