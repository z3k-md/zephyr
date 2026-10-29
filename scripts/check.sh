#!/usr/bin/env bash
# The fast pre-push check: what Checks runs, but only for the half of the app this branch touches
# (see scripts/changes.sh), compared with origin/develop plus uncommitted and untracked files.
# On Windows a Rust change is also linted for macOS, which catches most Mac-only breakage
# without a Mac.
#
#   scripts/check.sh           what changed
#   scripts/check.sh --all     everything
#   scripts/check.sh --no-mac  skip the macOS lint
set -euo pipefail
cd "$(dirname "$0")/.."

all=false
mac=true
for arg in "$@"; do
  case "$arg" in
    --all) all=true ;;
    --no-mac) mac=false ;;
    *)
      echo "unknown option: $arg" >&2
      exit 2
      ;;
  esac
done

# rustup's proxies, so rust-toolchain.toml picks the compiler even if another Rust is on PATH.
[ -d "$HOME/.cargo/bin" ] && export PATH="$HOME/.cargo/bin:$PATH"

if $all; then
  rust=true
  web=true
else
  base=$(git merge-base HEAD origin/develop 2> /dev/null || echo HEAD)
  eval "$(
    {
      git diff --name-only "$base"
      git ls-files --others --exclude-standard
    } | bash scripts/changes.sh
  )"
fi
echo "rust=$rust web=$web"

started=$SECONDS
step() {
  echo
  echo "== $1"
}

# vue-tsc under Bun skips .vue files without saying so, so the typecheck wants a real Node, and
# `bun run` stands Bun in for a missing one.
node_ok=false
if [ "$(node -e 'process.stdout.write(typeof Bun)' 2> /dev/null)" = undefined ]; then
  node_ok=true
fi

# Near instant when nothing changed, and keeps a worktree's packages in step with bun.lock.
bun install --frozen-lockfile --silent

step "Prettier"
bunx prettier --check .

if $web; then
  step "Typecheck"
  if $node_ok; then
    node node_modules/vue-tsc/bin/vue-tsc.js --noEmit
  else
    echo "No Node on PATH: .vue files aren't type-checked here (Checks does it). Install Node 22+."
    bunx vue-tsc --noEmit
  fi
  step "Frontend tests"
  bun run test:web
fi

if $node_ok && { $all || ! git diff --quiet "${base:-HEAD}" -- scripts; }; then
  step "Release script tests"
  node --test scripts/*.test.mjs
fi

if $rust; then
  if $web || [ ! -f dist/index.html ]; then
    step "Frontend build"
    bun run vite build
  fi
  manifest=src-tauri/Cargo.toml
  step "rustfmt"
  cargo fmt --manifest-path $manifest --check
  step "Clippy"
  cargo clippy --manifest-path $manifest --all-targets --locked -- -D warnings
  step "Rust tests"
  cargo test --manifest-path $manifest --locked

  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) on_windows=true ;;
    *) on_windows=false ;;
  esac
  if $mac && $on_windows; then
    step "Clippy for macOS"
    target=aarch64-apple-darwin
    rustup target list --installed | grep -qx $target || rustup target add $target
    # cargo clippy never links, so the two crates that compile C for macOS only need their build
    # scripts to succeed: a stand-in compiler and archiver that create empty outputs do. cc runs a
    # compiler through an sccache RUSTC_WRAPPER, which can't run the stand-in, so it's left out.
    stub="$PWD/src-tauri/target/mac-stub"
    mkdir -p "$stub"
    printf '@echo off\r\n:loop\r\nif "%%~1"=="" exit /b 0\r\nif "%%~1"=="-o" (type nul > "%%~2" & shift)\r\nshift\r\ngoto loop\r\n' > "$stub/cc.cmd"
    printf '@echo off\r\nif not "%%~2"=="" type nul > "%%~2"\r\nexit /b 0\r\n' > "$stub/ar.cmd"
    RUSTC_WRAPPER="" \
      CC_aarch64_apple_darwin="$(cygpath -w "$stub/cc.cmd")" \
      CXX_aarch64_apple_darwin="$(cygpath -w "$stub/cc.cmd")" \
      AR_aarch64_apple_darwin="$(cygpath -w "$stub/ar.cmd")" \
      cargo clippy --manifest-path $manifest --target $target --all-targets --locked -- -D warnings
  fi
fi

echo
echo "All checks passed in $((SECONDS - started))s."
