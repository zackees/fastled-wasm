#!/bin/bash

INSTALL_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
mkdir -p "$INSTALL_DIR" && \
curl -L -o fastled-linux-x64.zip https://github.com/zackees/fastled-wasm/releases/latest/download/fastled-linux-x64.zip && \
unzip -o fastled-linux-x64.zip && \
rm -f fastled-linux-x64.zip && \
mv fastled "$INSTALL_DIR/" && \

chmod +x "$INSTALL_DIR/fastled"