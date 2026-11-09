---
title: Scopes
summary: Narrow bar search with a !word before or after the query.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.0.0
code:
  - src-tauri/src/query.rs
  - src-tauri/src/settings.rs
  - src-tauri/src/suggest.rs
gaps:
  - Frecency for opened settings
  - Enumerate installed macOS settings extensions
---

A `!word` before or after the text narrows the search. Unknown `!words` stay as search text. Without a scope, a clear match for a setting, file or bookmark shows as at most two rows under the other results, and Enter never picks one unless you arrow to it. Only files you've opened from Zephyr before appear unscoped.

## Use it

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

On a scoped query, once you type something, Enter takes the first row. The empty `!set` scope lists Zephyr Settings first, then the OS settings you open most often.

## Keys

| Key   | Does                                      |
| ----- | ----------------------------------------- |
| Enter | On a scoped query, open the first match   |
| ↑ ↓   | Move between rows when results are listed |

## Settings

`!set` searches Windows Settings, Control Panel and macOS System Settings pages, plus every section in Zephyr Settings. Opening a page from the bar ranks it higher next time.
