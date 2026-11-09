#!/usr/bin/env bash
# Which halves of the app a set of changed paths touches, so Checks and scripts/check.sh can skip
# the half a change can't affect. Reads paths on stdin, prints `rust=true|false`, `web=true|false`
# and `docs=true|false`. Anything that changes the toolchain or the checks themselves turns on both
# halves. Any change to either half turns on docs too, since code changes can leave a page stale.
set -euo pipefail

rust=false
web=false
docs=false
while IFS= read -r path; do
  case "$path" in
    "") ;;
    .github/* | rust-toolchain.toml | package.json | bun.lock | scripts/changes.sh | scripts/check.sh)
      rust=true
      web=true
      ;;
    docs/* | README.md | extension/* | scripts/docs-check*.mjs) docs=true ;;
    src-tauri/*) rust=true ;;
    src/* | index.html | vite.config.ts | vitest.config.ts | tsconfig*.json) web=true ;;
  esac
done

if $rust || $web; then
  docs=true
fi

echo "rust=$rust"
echo "web=$web"
echo "docs=$docs"
