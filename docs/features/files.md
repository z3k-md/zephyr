---
title: Files
summary: Search file and folder names under your home folder; open, preview and drag out.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.3.0
code:
  - src-tauri/src/files.rs
  - src-tauri/src/commands/files.rs
  - src/bar/FilePreviewPane.vue
  - src/bar/dragOut.ts
gaps:
  - Search inside file contents
  - Ranking by recently modified
---

`!f budget` searches file and folder names under your home folder (or folders you add in Settings). Hidden folders, `.gitignore`d files, `node_modules` and build caches are skipped. Files you opened from Zephyr before can also appear in unscoped search, at most two rows below other results.

## Use it

Enter opens the top match. Use the action menu or the keys below for other actions. Drag a file row out into another app to drop the file there.

## Keys

| Key                 | Does                             |
| ------------------- | -------------------------------- |
| Enter               | Open the selected file or folder |
| Ctrl+Enter (⌘Enter) | Show it in Explorer or Finder    |
| Ctrl+Shift+C        | Copy its path                    |
| Shift+Space         | Preview beside the results       |
| Esc                 | Close the file preview           |

## Settings

**Settings > Files** — add folders to search, skip folders, rebuild the index, or return to indexing only your home folder.
