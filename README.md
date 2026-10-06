# Zephyr

A dispatch bar for Windows and macOS. Summon it over any app, type once, and send that text to Google, ChatGPT, Wikipedia, PubMed, or a destination you added, or open an installed app.

`Alt+Space` opens the bar (`Command+Space` on a Mac keyboard). Enter searches the armed destination. `Ctrl+1` through `Ctrl+8` send the same text to a pinned destination. `!` filters destinations, and `crispr !pm` searches PubMed directly.

Typing the start of an app's name (`chr`, `excel`, or initials like `vsc`) selects that app, and Enter opens it; longer free text still goes to the armed destination. A bang or `Ctrl+1`–`Ctrl+8` always sends the text to the web, and Zephyr remembers that choice for the same text next time. `!app` searches only apps.

`!set` searches settings: Windows Settings pages and Control Panel tools, macOS System Settings panes, and Zephyr's own settings (`!set dark mode`, `!set bluetooth`, `display !set`, `!set zephyr shortcut`). Enter opens the top match. Without `!set`, a clear match shows as at most two rows under the other results, and Enter never picks one unless you arrow to it.

**Ask AI** answers in the bar. Press Tab to arm it, or type `!ai` before a question; the answer streams in and Enter copies it. Pick the provider in Settings > AI: a local Ollama or LM Studio server (found automatically), or your own Anthropic, OpenAI, OpenRouter or custom key. Keys are kept in the system keychain, and requests go straight from Zephyr to the provider.

Math, unit conversions and time zones answer in the top row as you type (`12 * 7`, `5 km to miles`, `time in tokyo`, `3pm pst to cet`); Enter copies the answer. Ctrl+K (⌘K on a Mac) lists more actions for the selected row: send the text to another destination, copy the link, text or path, or show a file in its folder.

A destination's template can be a URL, an app link such as `obsidian://`, or a file path. Besides `{query}` it can use `{clipboard}`, `{date offset=+1d format=%Y-%m-%d}` and `{argument name="lang" default="en"}`, with pipes like `{query | trim | lowercase}` or `| raw` to skip URL encoding. A template without the query opens as soon as you type its bang. Any destination can show autocomplete from a URL that returns JSON: give the URL with `{query}` and the path to the suggestions (`1` for OpenSearch-style lists, or something like `items.*.title`). Settings > Destinations copies all destinations as JSON and imports them back.

Other apps and scripts can drive Zephyr with links: `zephyr://open?q=text` shows the bar with text typed in, `zephyr://dispatch?d=pubmed&q=crispr` sends it to a destination, and `zephyr://ask?q=...` asks AI. On macOS the link scheme comes from the installed app.

**Clipboard history** (macOS for now): press ⌘⇧V or type `!clip` to search everything you've copied, including text, links, colors, images and files. Text in copied screenshots is searchable too. Enter pastes into the app you're in, ⌘Enter copies, ⇧Enter pastes as plain text, and ⌘K shows more. History stays on this computer, encrypted with a key in the system keychain, and copies from password managers are never recorded.

**Notes**: press ⌥⌘N (Ctrl+Alt+N on Windows) for a small always-on-top notes window with rich text, or type `!note` to find a note or start one. Notes are HTML files in Zephyr's settings folder.

**Claude jobs**: type `!claude <alias> <task>` from any app, and your own `claude` CLI works on it in the background in a project folder you've registered in Settings > Claude. When it needs permission you get a notification and a card in the bar (⌘Y allow, ⌘⇧Y always, ⌘N deny). Commit and push always ask, and destructive git commands are always refused. `!claude` alone lists jobs; pick one and type to follow up.

**Typing test**: `!type` runs a Monkeytype-style test (15/30/60 seconds or 10/25/50 words) with WPM, accuracy, consistency and personal bests.

Reopening the bar soon after closing it returns to the view you were in. The window is set in Settings > General. Esc steps back to the main bar, and with nothing typed the bar offers Claude jobs, clipboard history and notes.

`!f` (or `!file`, `!files`) searches file and folder names under your home folder (`!f budget`, `!f taxes 2025 return`). Enter opens the top match with its default app, `Ctrl+Enter` shows it in Explorer or Finder, and `Ctrl+Shift+C` copies its path. Hidden folders, `.gitignore`d files, `node_modules` and build caches are skipped; Settings > Files picks other folders or leaves some out. Without `!f`, only files you've opened from Zephyr before can appear, sharing the same two rows as settings.

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

Releases are cut on demand: run the **Release** workflow from the Actions tab. It runs the checks on Windows and macOS, and if [semantic-release](https://semantic-release.gitbook.io/) finds a `feat:` or `fix:` commit since the last tag, it bumps the version, tags it, and creates a draft GitHub release. Windows and macOS (universal) installers are then built, signed for the updater, and uploaded. A final job writes `latest.json` and publishes the release, and installed copies pick it up from there.

Repository secrets:

| Secret                                                                                                                     | Purpose                                                                                                                                                                                           |
| -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TAURI_PRIVATE_KEY`, `TAURI_KEY_PASSWORD`                                                                                  | Updater signing key. It must match the `pubkey` in `src-tauri/tauri.conf.json`, or installed copies will reject updates.                                                                          |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Optional. With them, the macOS app is signed with a Developer ID and notarized. Without them it is ad-hoc signed: first launch needs right-click > Open, and updates still install automatically. |

For a local signed build, copy `.env.example` to `.env`, fill in the signing key, and run `bun run build`.
