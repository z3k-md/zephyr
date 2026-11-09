---
title: Apps and windows
summary: Launch apps and Steam games from the bar; switch running apps and their windows on Windows.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.1.0
code:
  - src-tauri/src/apps.rs
  - src-tauri/src/running.rs
  - src-tauri/src/steam.rs
  - src-tauri/src/icons.rs
  - src-tauri/src/commands/apps.rs
  - src/bar/appIcons.ts
gaps:
  - List and switch to an app's open windows on macOS, as on Windows
---

Typing the start of an app's name (`chr`, `excel`, or initials like `vsc`) selects that app, and Enter opens it. Use `!app` to search only apps and installed Steam games. On Windows a running app's row switches to its window, and typing part of a window title finds it. On macOS, Enter still activates a running app, but window lists and switching are not available yet.

## Use it

Pick an app row and press Enter to launch it or bring it forward. If the text also matches Google, choosing the Google row once sends it to the web instead; Zephyr remembers that choice for the same text next time.

On Windows, open Ctrl+K on a running app to switch to another window, open a new one, close a window or quit the app. On macOS, Ctrl+K on a running app can quit it.

## Keys

| Key                  | Does                              |
| -------------------- | --------------------------------- |
| Enter                | Launch or switch to the app       |
| Ctrl+K (⌘K on macOS) | More actions for the selected row |

On Windows, the action menu can switch to another window, open a new one, close a window or quit the app. On macOS it can quit a running app.

## Settings

Apps are indexed automatically. Steam games appear when Steam is installed.
