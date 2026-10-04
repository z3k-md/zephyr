# Zephyr

A dispatch bar for Windows and macOS. Summon it over any app, type once, and send that text to Google, ChatGPT, Wikipedia, PubMed, or a destination you added.

`Alt+Space` opens the bar (`Command+Space` on a Mac keyboard). Enter searches the armed destination. `Ctrl+1` through `Ctrl+8` send the same text to a pinned destination. `!` filters destinations, and `crispr !pm` searches PubMed directly.

## Install

Download the latest installer from [Releases](https://github.com/z3k-md/zephyr/releases/latest): the `-setup.exe` for Windows, or the `.dmg` for macOS. Installed copies check for updates at launch and every six hours, and install them automatically. Settings and the tray menu also have **Check for updates**.

## Develop

- [Rust](https://www.rust-lang.org/tools/install)
- [Bun](https://bun.sh/)

```bash
bun install
bun dev
```

`bun run test` runs the Rust tests. `bun run lint` runs every check CI runs: Prettier, vue-tsc, rustfmt, and Clippy with warnings as errors. `bun run format` fixes formatting.

## Release

Every push to `main` runs the checks on Windows and macOS. If [semantic-release](https://semantic-release.gitbook.io/) finds a `feat:` or `fix:` commit since the last tag, it bumps the version, tags it, and creates a draft GitHub release. Windows and macOS (universal) installers are then built, signed for the updater, and uploaded. A final job writes `latest.json` and publishes the release, and installed copies pick it up from there.

Repository secrets:

| Secret                                                                                                                     | Purpose                                                                                                                                                                                           |
| -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TAURI_PRIVATE_KEY`, `TAURI_KEY_PASSWORD`                                                                                  | Updater signing key. It must match the `pubkey` in `src-tauri/tauri.conf.json`, or installed copies will reject updates.                                                                          |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Optional. With them, the macOS app is signed with a Developer ID and notarized. Without them it is ad-hoc signed: first launch needs right-click > Open, and updates still install automatically. |

For a local signed build, copy `.env.example` to `.env`, fill in the signing key, and run `bun run build`.
