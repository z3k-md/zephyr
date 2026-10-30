# Zephyr

[![Release](https://github.com/z3k-md/zephyr/actions/workflows/release.yml/badge.svg)](https://github.com/z3k-md/zephyr/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/z3k-md/zephyr?sort=semver)](https://github.com/z3k-md/zephyr/releases/latest)

A launcher bar for Windows and macOS. Summon it over any app and type to open an app, a setting, a file or a link, or search Google.

`Alt+Space` opens the bar (`Command+Space` on a Mac keyboard). Typing the start of an app's name (`chr`, `excel`, or initials like `vsc`) selects that app, and Enter opens it; other free text is searched on Google, and a link (`example.com/docs`) opens directly. Picking the Google row for text that matched an app sends it to the web, and Zephyr remembers that choice for the same text next time. `!app` searches only apps.

`!set` searches settings: Windows Settings pages and Control Panel tools, macOS System Settings panes, and Zephyr's own settings (`!set dark mode`, `!set bluetooth`, `display !set`, `!set zephyr shortcut`). Enter opens the top match. Without `!set`, a clear match shows as at most two rows under the other results, and Enter never picks one unless you arrow to it.

Shift+Enter on a Google search opens it in Google's AI Mode instead of the usual results.

Math, unit conversions and time zones answer in the top row as you type (`12 * 7`, `5 km to miles`, `time in tokyo`, `3pm pst to cet`); Enter copies the answer. Ctrl+K (⌘K on a Mac) lists more actions for the selected row: copy the link, text or path, or show a file in its folder.

Other apps and scripts can drive Zephyr with links: `zephyr://open?q=text` shows the bar with text typed in, and `zephyr://search?q=crispr` searches the web for it (or opens it, when it is a web address). Links never launch apps, open files or reach the shell. On macOS the link scheme comes from the installed app.

**Clipboard history**: press ⌘⇧V (Alt+Shift+V on Windows) or type `!clip` to search everything you've copied, including text, links, colors, images and files. Text in copied screenshots is searchable too. Enter pastes into the app you're in, Ctrl+Enter (⌘Enter) copies, Shift+Enter pastes as plain text, and Ctrl+K (⌘K) shows more. History stays on this computer, encrypted with a key in the system keychain (Credential Manager on Windows), and copies from password managers are never recorded.

**Notes**: press ⌥⌘N (Ctrl+Alt+N on Windows) for a small always-on-top notes window with rich text, or type `!note` to find a note or start one. Notes are HTML files in Zephyr's settings folder.

**Typing test**: `!type` runs a Monkeytype-style test (15/30/60 seconds or 10/25/50 words) with WPM, accuracy, consistency and personal bests.

Reopening the bar soon after closing it returns to the view you were in. The window is set in Settings > General. Esc steps back to the main bar, and with nothing typed the bar offers clipboard history and notes.

`!f` (or `!file`, `!files`) searches file and folder names under your home folder (`!f budget`, `!f taxes 2025 return`). Enter opens the top match with its default app, `Ctrl+Enter` shows it in Explorer or Finder, and `Ctrl+Shift+C` copies its path. Hidden folders, `.gitignore`d files, `node_modules` and build caches are skipped; Settings > Files picks other folders or leaves some out. Without `!f`, only files you've opened from Zephyr before can appear, sharing the same two rows as settings.

## Install

Download the latest installer from [Releases](https://github.com/z3k-md/zephyr/releases/latest): the `-setup.exe` for Windows, or the `.dmg` for macOS. Installed copies check for updates at launch and every six hours, and install them automatically. Settings and the tray menu also have **Check for updates**.

The Windows installer isn't Authenticode-signed, so SmartScreen warns about an unrecognized app the first time: choose **More info > Run anyway**. Updates are still verified against Zephyr's own updater key.

## Develop

- [Rust](https://www.rust-lang.org/tools/install) through rustup, which installs the version pinned in `rust-toolchain.toml` on the first build
- [Bun](https://bun.sh/)
- [Node](https://nodejs.org/) 22.14 or later, for vue-tsc (under Bun on Windows it skips `.vue` files) and the release script tests

```bash
bun install
bun dev
```

Before pushing, run `bun run check` (`scripts/check.sh`). It runs what Checks runs, but only for the half of the app your branch touches compared with `origin/develop`, uncommitted files included: Prettier always, vue-tsc and the frontend tests for frontend changes, rustfmt, Clippy and the Rust tests for Rust changes. Changes to the toolchain, `package.json` or the workflows check both. On Windows a Rust change is also linted for macOS (`--target aarch64-apple-darwin`; `cargo clippy` doesn't link, so no Mac SDK is needed), which catches most Mac-only breakage before CI does. `--all` checks everything and `--no-mac` skips the macOS lint.

`bun run lint` and `bun run test` still run every check unconditionally, and `bun run test:scripts` the release script tests. `bun run format` fixes formatting. A shared compile cache such as [sccache](https://github.com/mozilla/sccache) (`build.rustc-wrapper` in `~/.cargo/config.toml`) makes fresh checkouts and worktrees reuse each other's dependency builds.

The version in `package.json` and `src-tauri/Cargo.toml` is the last release's, for local builds. Releases stamp both from the tag, so they aren't bumped by hand. `rust-version` in `Cargo.toml` follows the toolchain in `rust-toolchain.toml`; a test fails if they differ.

## Branches

Work lands on `develop`. Checks run on Windows and macOS for every push to `develop` and every pull request to `develop` or `main`, next to the release script tests and a lint of the workflow files. A running check on `develop` always finishes; of the pushes that arrive meanwhile, only the newest is checked next, against the last commit whose checks passed, so nothing goes unchecked. Like `scripts/check.sh`, a push skips the Rust or frontend half when it doesn't touch it, and the platform-independent checks (formatting, vue-tsc, frontend tests) run on macOS only. Windows builds on a Dev Drive. CodeQL scans `develop` on each push (Rust weekly), and the Prune caches workflow keeps only the newest cargo cache per job and OS. Pushes to `main` aren't checked again: `main` moves only when `develop` is promoted, by fast-forwarding it once `develop`'s checks are green:

```bash
git push origin develop:main
```

`main` requires both checks to pass and can't be deleted. Dependabot opens version updates as pull requests to `develop`.

## Release

Releases are cut on demand from `main`: run the **Release** workflow from the Actions tab. It has no checks of its own, since `main` only holds commits already checked on `develop`; run from any other branch, it fails. If [semantic-release](https://semantic-release.gitbook.io/) finds a `feat:` or `fix:` commit since the last tag, it tags the next version and creates a draft GitHub release with its notes. Nothing is committed back to `main`: the build stamps the version from the tag, and the notes on each [GitHub release](https://github.com/z3k-md/zephyr/releases) are the changelog. Windows and macOS (Apple silicon only) installers are then built, signed for the updater, and uploaded. A final job writes `latest.json` and publishes the release, and installed copies pick it up from there. With no `feat:` or `fix:` commit to release, the run ends with a warning and changes nothing.

Repository secrets:

| Secret                                                                      | Purpose                                                                                                                                                                                                                                                                               |
| --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TAURI_PRIVATE_KEY`, `TAURI_KEY_PASSWORD`                                   | Updater signing key. It must match the `pubkey` in `src-tauri/tauri.conf.json`, or installed copies will reject updates.                                                                                                                                                              |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | Optional. A self-signed certificate for the macOS app, so it keeps the same identity and its permissions across updates. Without them it is ad-hoc signed and asks for permissions again after every update. Either way it isn't notarized, so first launch needs right-click > Open. |

For a local signed build, copy `.env.example` to `.env`, fill in the signing key, and run `bun run build`.

### Windows code signing

Windows builds aren't Authenticode-signed yet, hence the SmartScreen warning; the updater signature above is separate and doesn't affect it. Signing needs a code signing certificate, and then one of:

- A certificate in the build machine's store: set `bundle.windows.certificateThumbprint` (with `digestAlgorithm` and `timestampUrl`) in `src-tauri/tauri.conf.json`, and import the `.pfx` from a secret in the Windows build job before `tauri-action` runs.
- [Azure Trusted Signing](https://learn.microsoft.com/azure/trusted-signing/), which needs no certificate file: set `bundle.windows.signCommand` to a `trusted-signing-cli` command and give the job the Azure credentials as secrets.

Neither is configured, since either one fails the build without its certificate or credentials. A new certificate still builds SmartScreen reputation over time; an EV certificate or Trusted Signing skips most of that.
