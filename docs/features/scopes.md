---
title: Scopes
summary: Narrow bar search with a scope word before or after the query.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.0.0
code:
  - src-tauri/src/query.rs
  - src-tauri/src/settings.rs
  - src-tauri/src/suggest.rs
gaps:
  - Installed macOS settings extensions in !set
---

A `!word` before or after the text narrows the search. Unknown `!words` stay as search text. Without a scope, a clear match for a setting, file or bookmark shows as at most two rows under the other results, and Enter never picks one unless you arrow to it. Only files you've opened from Zephyr before appear unscoped.

## Scopes

| Scope         | Searches                                                                                                                |
| ------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `!app`        | Apps and installed Steam games                                                                                          |
| `!set`        | Windows Settings and Control Panel, macOS System Settings, and Zephyr's own settings (`!set bluetooth`, `display !set`) |
| `!f`, `!file` | File and folder names under your home folder                                                                            |
| `!bm`         | Bookmarks from Chromium browsers                                                                                        |
| `!note`       | Notes, or a new one                                                                                                     |
| `!clip`       | Clipboard history                                                                                                       |
| `!sh`, `>`    | The shell                                                                                                               |
| `!type`       | The typing test                                                                                                         |
| `!emoji`      | Emoji and symbols (`:fire`, `:arrow`, `fire !emoji`)                                                                    |

In a scope with text typed, Enter opens the first row. An empty `!set` lists Zephyr's settings first, then the system settings you open most. Settings you open from the bar rank higher next time.
