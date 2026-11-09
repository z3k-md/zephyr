---
title: Emoji and symbols
summary: Search emoji and symbols, then paste or copy from the bar.
status: shipped
group: utilities
platforms: [windows, macos]
code:
  - src/bar/emoji/**
  - src/bar/EmojiGrid.vue
  - src-tauri/src/commands/emoji.rs
  - scripts/emoji.mjs
---

`:fire`, `:arrow`, or `!emoji` (for example `fire !emoji`) opens the emoji grid in the bar.
Recents and your skin tone are remembered.

## Use it

Type to search names and keywords. Enter pastes into the app in front. Ctrl+Enter (⌘Enter)
copies. Tab cycles the skin tone on supported emoji. Arrow keys and Page Up/Down move the
selection.

## Keys

| Key                 | Does               |
| ------------------- | ------------------ |
| ↑ / ↓ / ← / →       | Move the selection |
| Page Up / Page Down | Jump by page       |
| Enter               | Paste              |
| Ctrl+Enter (⌘Enter) | Copy               |
| Tab                 | Cycle skin tone    |

## Settings

No dedicated section. Skin tone and recents are stored in your settings file automatically.
