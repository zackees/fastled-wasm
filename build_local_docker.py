#!/usr/bin/env -S uv run

# Requirements:
# - python-dateutil

# pyproject.toml
# [project]
# dependencies = [
#   "python-dateutil"
# ]

import os
import shutil
import subprocess

def _exec(cmd: str) -> int:
    print(f"$ {cmd}")
    return subprocess.call(cmd, shell=True)

# Step 1: Stop containers and remove images
_exec("docker compose down --remove-orphans --rmi all")

# Step 2: Remove all dangling images
_exec("docker image prune -f")

# Step 3: Remove all images matching niteris/fastled-wasm
images = subprocess.check_output(
    "docker images --format '{{.Repository}} {{.ID}}'",
    shell=True, text=True
)

for line in images.strip().splitlines():
    repo, image_id = line.split()
    if repo == "niteris/fastled-wasm":
        _exec(f"docker rmi {image_id}")

# Step 4: Optionally remove volumes
# _exec("docker volume prune -f")  # Uncomment if you want to purge volumes too


# Step 5: Rebuild images
_exec("docker compose build")
