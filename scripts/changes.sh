#!/usr/bin/env bash
# Which halves of the app a set of changed paths touches, so Checks and scripts/check.sh can skip
# the half a change can't affect. Reads paths on stdin, prints `rust=true|false` and
# `web=true|false`. Anything that changes the toolchain or the checks themselves turns on both.
set -euo pipefail

rust=false
web=false
while IFS= read -r path; do
  case "$path" in
    "") ;;
    .github/* | rust-toolchain.toml | package.json | bun.lock | scripts/changes.sh | scripts/check.sh)
      rust=true
      web=true
      ;;
    src-tauri/*) rust=true ;;
    src/* | index.html | vite.config.ts | vitest.config.ts | tsconfig*.json) web=true ;;
  esac
done

echo "rust=$rust"
echo "web=$web"
