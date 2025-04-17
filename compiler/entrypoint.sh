#!/usr/bin/env bash

# load emsdk environment
source /emsdk/emsdk_env.sh
export PATH="$PATH:/emsdk/upstream/bin"

# initialize runtime
python init_runtime.py

# only do the final prewarm if RUNTIME_PREWARM is set to "1"
if [[ "${RUNTIME_PREWARM:-0}" == "1" ]]; then
  ./final_prewarm.sh
fi

# hand off to the main command
exec "$@"
