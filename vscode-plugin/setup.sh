#!/bin/bash

# FastLED WASM VS Code Extension Setup Script

echo "Setting up FastLED WASM VS Code Extension..."

# Check if Node.js is installed
if ! command -v node &> /dev/null; then
    echo "Error: Node.js is not installed. Please install Node.js first."
    exit 1
fi

# Check if npm is installed
if ! command -v npm &> /dev/null; then
    echo "Error: npm is not installed. Please install npm first."
    exit 1
fi

# Install dependencies
echo "Installing dependencies..."
npm install

# Compile TypeScript
echo "Compiling TypeScript..."
npm run compile

echo "Setup complete!"
echo ""
echo "To run the extension in development mode:"
echo "1. Open this folder in VS Code"
echo "2. Press F5 to launch a new Extension Development Host window"
echo "3. Test the FastLED commands in the new window"
echo ""
echo "To package the extension:"
echo "1. Install vsce: npm install -g vsce"
echo "2. Package: vsce package"
echo ""
echo "Make sure you have the FastLED WASM compiler installed:"
echo "pip install fastled" 