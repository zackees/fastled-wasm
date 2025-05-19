#!/bin/bash

if [ "$RENDER" != "true" ]; then
  echo "Skipping finalprewarm..."
  exit 0
fi

uv run -m fastled_wasm_compiler.cli_update_from_master

