---
title: Hotkeys and aliases
summary: Global shortcuts and bar aliases for apps, settings, bookmarks, launch groups and more.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.0.0
code:
  - src-tauri/src/shortcut.rs
  - src-tauri/src/bindings.rs
  - src-tauri/src/commands/shortcuts.rs
  - src/settings/ShortcutsSection.vue
  - src/components/ShortcutRecorder.vue
---

Any command, app, bookmark, setting or launch group can have a global hotkey or an alias typed in the bar. The bar's summon shortcut is also configured here.

## Use it

Set bindings in **Settings > Shortcuts**, or choose **Set hotkey or alias** from Ctrl+K on a row. Aliases are short words you type in the bar; hotkeys work even when the bar is closed.

## Keys

| Key                  | Does on a row                               |
| -------------------- | ------------------------------------------- |
| Ctrl+K (⌘K on macOS) | Open actions, including Set hotkey or alias |

## Settings

**Settings > Shortcuts** — summon shortcut, aliases, hotkeys for utilities (clipboard, notes, color picker and others), and per-item bindings.
