# Develop

- [Rust](https://www.rust-lang.org/tools/install) through rustup, which installs the version pinned in [`rust-toolchain.toml`](https://github.com/z3k-md/zephyr/blob/develop/rust-toolchain.toml) on the first build
- [Bun](https://bun.sh/)
- [Node](https://nodejs.org/) 22.14 or later, for vue-tsc (under Bun on Windows it skips `.vue` files) and the release script tests

```bash
bun install
bun dev
```

In dev, every window's console warnings, errors and uncaught exceptions are also written to `logs/frontend.log`, which is cleared each time the dev server starts.

Before pushing, run `bun run check` (`scripts/check.sh`). It runs what Checks runs, but only for the half of the app your branch touches compared with `origin/develop`, uncommitted files included: Prettier always, vue-tsc and the frontend tests for frontend changes, rustfmt, Clippy and the Rust tests for Rust changes. Changes to the toolchain, `package.json` or the workflows check both. On Windows a Rust change is also linted for macOS (`--target aarch64-apple-darwin`; `cargo clippy` doesn't link, so no Mac SDK is needed), which catches most Mac-only breakage before CI does. `--all` checks everything and `--no-mac` skips the macOS lint.

`bun run lint` and `bun run test` still run every check unconditionally, and `bun run test:scripts` the release script tests. `bun run format` fixes formatting. A shared compile cache such as [sccache](https://github.com/mozilla/sccache) (`build.rustc-wrapper` in `~/.cargo/config.toml`) makes fresh checkouts and worktrees reuse each other's dependency builds.

The version in `package.json` and `src-tauri/Cargo.toml` is the last release's, for local builds. Releases stamp both from the tag, so they aren't bumped by hand. `rust-version` in `Cargo.toml` follows the toolchain in `rust-toolchain.toml`; a test fails if they differ.

Docs: `bun run docs:dev` serves the site locally; see [How the docs work](/contributing/docs).
