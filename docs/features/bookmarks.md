---
title: Bookmarks
summary: Search Chromium bookmarks and open them from the bar.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.6.0
code:
  - src-tauri/src/bookmarks.rs
---

`!bm` searches bookmarks from Chrome, Edge, Brave, Vivaldi, Arc and other Chromium browsers. Without a scope, bookmark matches can appear as at most two rows below other results, and Enter does not pick them unless you arrow there.

Type `!bm` plus words from the bookmark title or URL. Enter opens the page in your default browser. With the [browser extension](/features/browser-extension) installed, opening a bookmark switches to an existing tab when the page is already open instead of opening a duplicate.

## Keys

| Key   | Does              |
| ----- | ----------------- |
| Enter | Open the bookmark |

## Settings

Zephyr reads each browser's bookmark file from its profile folder when you search.
