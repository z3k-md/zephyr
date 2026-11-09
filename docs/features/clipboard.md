---
title: Clipboard history
summary: Search everything you've copied and paste it into the app in front.
status: shipped
group: utilities
platforms: [windows, macos]
since: 2.4.0
code:
  - src-tauri/src/clipboard/**
  - src-tauri/src/commands/clipboard.rs
  - src-tauri/src/clip_cycle.rs
  - src-tauri/src/keyhook.rs
  - src/bar/ClipboardView.vue
  - src/clipcycle/**
gaps:
  - Sync across computers (waits on Sync)
---

⌘⇧V (Alt+Shift+V on Windows) or `!clip` opens clipboard history from the bar. Search text,
links, colors, images, files, and text recognized inside screenshots.

## Use it

Pick a row and Enter pastes into the app in front. Ctrl+Enter (⌘Enter) copies without pasting.
Shift+Enter pastes as plain text. Tab cycles the type filter. Ctrl+K lists more actions on a row.

On Windows, hold Ctrl after Ctrl+C and press ↑ or ↓ to step through earlier copies without
opening the bar. Release Ctrl to copy the selected item, press V to paste it, or Esc to cancel.
The popup shows a short preview of each item.

## Keys

| Key                 | Does                |
| ------------------- | ------------------- |
| ↑ / ↓               | Move the selection  |
| Enter               | Paste               |
| Ctrl+Enter (⌘Enter) | Copy                |
| Shift+Enter         | Paste as plain text |
| Tab                 | Cycle type filter   |
| Ctrl+K (⌘K)         | Row actions         |

### Clipboard cycler (Windows)

| Key                            | Does                                  |
| ------------------------------ | ------------------------------------- |
| Ctrl+C, then ↑ / ↓ (Ctrl held) | Step through earlier copies           |
| Release Ctrl                   | Copy the selected item                |
| V (while cycling)              | Paste it                              |
| Esc                            | Cancel without changing the clipboard |

## Settings

Settings > Clipboard turns history on or off, sets the global shortcut, retention, and whether
to search text inside copied images. Windows can enable **Ctrl+C, then ↑ steps through history**.
Add app names or bundle ids under **Never record from**; password managers are ignored by default.
**Allow pasting** grants Accessibility so Zephyr can paste straight into other apps (macOS).

## Privacy

History stays on this computer. Items are encrypted at rest with a key in the system keychain
(Credential Manager on Windows). Copies from password managers and other private clipboard flags
are never recorded.
