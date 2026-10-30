# Zephyr

[![Release](https://github.com/z3k-md/zephyr/actions/workflows/release.yml/badge.svg)](https://github.com/z3k-md/zephyr/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/z3k-md/zephyr?sort=semver)](https://github.com/z3k-md/zephyr/releases/latest)

A launcher bar for Windows and macOS. Summon it over any app and type to open an app, a setting, a file or a link, or search Google.

## Using the bar

`Alt+Space` opens the bar (`Command+Space` on a Mac keyboard). Typing the start of an app's name (`chr`, `excel`, or initials like `vsc`) selects that app, and Enter opens it; other free text is searched on Google, and a link (`example.com/docs`) opens directly. Picking the Google row for text that matched an app sends it to the web, and Zephyr remembers that choice for the same text next time. Shift+Enter on a search opens it in Google's AI Mode; on a browser, it opens a private window.

- Alt+1..9 (⌘1..9) runs the Nth row; hold the key to see the numbers. → or End accepts the faint type-ahead.
- Ctrl+K (⌘K) lists more actions for the selected row: copy the link, text or path, show a file in its folder, quit an app, or give it a hotkey or alias.
- Ctrl+drag (⌘-drag) moves the bar, with a snap grid; it reopens where you left it.
- Ctrl+S (⌘S) opens Settings. Esc steps back to the main bar.
- The empty bar shows recent searches, recent apps, the utilities you use most, and your IP address.
- Reopening the bar soon after closing it returns to the view you were in (Settings > General).
- The footer shows CPU, GPU and memory, network trouble and the weather; click any of them for more.

### Scopes

A `!word` before or after the text narrows the search. Unknown `!words` stay as search text.

| Scope         | Searches                                                                                                                |
| ------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `!app`        | Apps and installed Steam games                                                                                          |
| `!set`        | Windows Settings and Control Panel, macOS System Settings, and Zephyr's own settings (`!set bluetooth`, `display !set`) |
| `!f`, `!file` | File and folder names under your home folder                                                                            |
| `!bm`         | Bookmarks from Chrome, Edge, Brave, Vivaldi, Arc and other Chromium browsers                                            |
| `!note`       | Notes, or a new one                                                                                                     |
| `!clip`       | Clipboard history                                                                                                       |
| `!sh`, `>`    | The shell                                                                                                               |
| `!type`       | The typing test                                                                                                         |

Without a scope, a clear match for a setting, file or bookmark shows as at most two rows under the other results, and Enter never picks one unless you arrow to it. Only files you've opened from Zephyr before appear unscoped.

### Answers

These answer in the top row as you type; Enter copies the answer.

- Math and units: `12 * 7`, `5 km to miles`
- Time zones: `time in tokyo`, `3pm pst to cet`
- Dates and timestamps: `unix`, `1760000000 unix`, `week`, `day of year`, `now iso`
- Network: `ip` shows local and public addresses; `port 3000` or `:3000` shows what's listening there and can quit it; `ports` lists every listening port
- Stocks and crypto: `TSLA`, `$aapl` or `aapl stock`, and `eth`, `btc` or `bitcoin` in any case, show a price card with its change and chart, from Yahoo Finance. Tab steps the chart through 1D, 5D, 1M and 1Y, and Ctrl+Enter opens the full quote.

### Apps, windows and launch groups

- On Windows a running app's row switches to its window, and typing part of a window title finds it. Ctrl+K switches to another window, opens a new one, closes it or quits the app. On macOS, Ctrl+K quits a running app.
- **Launch groups** open several apps, links and folders at once ("Coding", "Gaming"); apps already running come forward. Set them up in Settings > Launch groups and run them by name or hotkey.
- **Hotkeys and aliases**: any command, app, bookmark, setting or launch group can have a global hotkey or an alias typed in the bar (Settings > Shortcuts, or Ctrl+K > Set hotkey or alias).
- **Power**: `shutdown`, `restart`, `sleep`, `lock`, `sign out`, and on Windows `bios` (restart to UEFI). Shutting down, restarting and signing out need a second Enter. `shutdown in 30m` or `restart at 11pm` schedules one, and `cancel shutdown` stops it.
- With the [browser extension](extension/README.md), opening a bookmark switches to its tab when it's already open.

### Files

`!f budget` searches file and folder names under your home folder. Enter opens the top match, Ctrl+Enter shows it in Explorer or Finder, Ctrl+Shift+C copies its path, Shift+Space previews it, and you can drag a row out into another app. Hidden folders, `.gitignore`d files, `node_modules` and build caches are skipped; Settings > Files picks other folders or leaves some out.

### Utilities

Type a utility's name to open it.

- **Clipboard history**: ⌘⇧V (Alt+Shift+V on Windows) or `!clip` searches everything you've copied, including text, links, colors, images and files, and the text in copied screenshots. Enter pastes into the app you're in, Ctrl+Enter (⌘Enter) copies, Shift+Enter pastes as plain text. History stays on this computer, encrypted with a key in the system keychain (Credential Manager on Windows), and copies from password managers are never recorded.
- **Notes**: ⌥⌘N (Ctrl+Alt+N on Windows) opens a small rich-text note in its own window, and `!note` finds one or starts one. Notes are HTML files in Zephyr's settings folder, and open notes come back at the next start.
- **Shell**: `>` or `!sh` runs commands in the bar (Git Bash, PowerShell or WSL on Windows; your login shell on macOS). Ctrl+C stops a command, and Ctrl+Enter hands off to a real terminal.
- **Resources** (`top`): CPU, memory, GPU and network over the last 15 minutes, with apps above background processes. Ctrl+Backspace quits a process.
- **Network** (`ping`): what's wrong right now, with router, internet and DNS latency over time.
- **Speed test**: download, upload and lag under load, from Cloudflare.
- **Weather**: `weather` or `weather in tokyo`, from Open-Meteo.
- **Now playing**: what's playing shows at the end of the search row, with play, pause and skip. Ctrl+Space plays or pauses, and scrolling over it changes the volume.
- **Typing test**: `!type` runs a Monkeytype-style test (15/30/60 seconds or 10/25/50 words) with WPM, accuracy, consistency and personal bests.

### Links

Other apps and scripts can drive Zephyr with links: `zephyr://open?q=text` shows the bar with text typed in, and `zephyr://search?q=crispr` searches the web for it (or opens it, when it is a web address). Links never launch apps, open files or reach the shell. On macOS the link scheme comes from the installed app.

### Sync (in progress)

Settings > Sync signs in with Google, sets up end-to-end encryption keys and a recovery key, and approves new computers from one you already use. Syncing notes and clipboard history comes next; nothing syncs yet.

## Install

Download the latest installer from [Releases](https://github.com/z3k-md/zephyr/releases/latest): the `-setup.exe` for Windows, or the `.dmg` for macOS. Installed copies check for updates shortly after launch and whenever the bar opens (at most every few minutes), and install them automatically. Settings and the tray menu also have **Check for updates**.

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
