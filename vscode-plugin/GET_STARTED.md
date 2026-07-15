# 🚀 Get Started with FastLED WASM VS Code Extension

Welcome! This is the VS Code extension for the FastLED WASM compiler. Here's how to get up and running in 3 simple steps:

## ⚡ Quick Start (3 Steps)

### 1️⃣ Install Dependencies
```bash
npm install
```

### 2️⃣ Compile the Extension  
```bash
npm run compile
```

### 3️⃣ Test the Extension
1. Open this folder in VS Code
2. Press `F5` to launch Extension Development Host
3. In the new window, press `Ctrl+Shift+P` and type "FastLED"
4. Try "FastLED: Initialize New Project"

## 📋 Prerequisites

- ✅ Node.js (v16+)
- ✅ VS Code
- ✅ FastLED WASM Compiler: `pip install fastled`

## 📁 Key Files

- `package.json` - Extension configuration and commands
- `src/extension.ts` - Main extension code  
- `README.md` - Complete documentation
- `syntaxes/` - Arduino syntax highlighting
- `snippets/` - FastLED code snippets

## 🔧 Development

```bash
# Watch mode (auto-compile on changes)
npm run watch

# Package for distribution
npm install -g vsce && vsce package

# Install packaged extension  
code --install-extension fastled-wasm-1.0.0.vsix
```

## ❓ Need Help?

- 📖 [Full Documentation](README.md)
- ⚙️ [Installation Guide](INSTALL.md) 
- 📋 [Quick Setup](README_INSTALL.md)
- 🔄 [Changelog](CHANGELOG.md)

---

**Ready to build amazing LED projects!** ⚡🌈 