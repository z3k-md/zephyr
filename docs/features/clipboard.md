---
title: Clipboard history
summary: Search everything you've copied and paste it into the front app.
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
  - Clipboard sync
---

Alt+Shift+V (⌘⇧V) or `!clip` opens it. It searches copied text, links, colors, images and files,
and the text inside copied screenshots.

On Windows, the clipboard cycler steps through earlier copies without opening the bar, in a small
popup that previews each one.

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
