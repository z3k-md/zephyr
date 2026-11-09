---
title: Notes
summary: Rich-text notes in windows, searchable from the bar.
status: shipped
group: utilities
platforms: [windows, macos]
since: 2.4.0
code:
  - src-tauri/src/notes.rs
  - src-tauri/src/note_windows.rs
  - src-tauri/src/commands/notes.rs
  - src/notes/**
---

⌥⌘N (Ctrl+Alt+N on Windows) or `!note` opens a note or finds one in the bar. Notes are HTML
files in Zephyr's settings folder; open notes return after a restart. Pick a note or start a new
one with Enter from the bar. The global shortcut opens a small note window over your work. Each
note saves as you type.

## Keys

| Windows    | macOS | Does                             |
| ---------- | ----- | -------------------------------- |
| Ctrl+Alt+N | ⌥⌘N   | Open the notes shortcut (global) |

In the bar, ↑ / ↓ and Enter work like other result lists.

## Settings

Settings > Notes sets the global shortcut, opens the notes list, and reveals the notes folder on
disk.
