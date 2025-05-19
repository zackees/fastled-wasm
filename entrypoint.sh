#!/usr/bin/env bash

# load emsdk environment
source /emsdk/emsdk_env.sh
export PATH="$PATH:/emsdk/upstream/bin"

# only do the final prewarm if RUNTIME_PREWARM is set to "1"
if [[ "${RUNTIME_PREWARM:-0}" == "1" ]]; then
    uv run -m fastled_wasm_compiler.cli_update_from_master
fi

# hand off to the main command
exec "$@"
