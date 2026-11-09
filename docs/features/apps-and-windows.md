---
title: Apps and windows
summary: Launch apps and Steam games; switch running apps and windows on Windows.
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

Type the start of an app's name (`chr`, `excel`) or its initials (`vsc`); Enter opens it. `!app` searches only apps and installed Steam games.

On Windows, a running app's row switches to its window, and part of a window title finds that window. On macOS, Enter brings a running app forward.

## Keys

| Key         | Does                                                                                   |
| ----------- | -------------------------------------------------------------------------------------- |
| Enter       | Open the app, or switch to it                                                          |
| Ctrl+K (⌘K) | Windows: switch to another window, open a new one, close it, quit. macOS: quit the app |
