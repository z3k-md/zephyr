---
title: Now playing
summary: Control the current song from the search row and footer.
status: shipped
group: utilities
platforms: [windows, macos]
since: 2.6.0
code:
  - src-tauri/src/nowplaying/**
  - src-tauri/src/commands/media.rs
  - src/bar/NowPlayingStrip.vue
  - src/bar/nowPlaying.ts
---

What's playing appears at the end of the search row and in the footer when a supported app is
active. Click the strip for previous, play/pause, and next. Scroll over it to change that app's
volume (mixer level on Windows, app volume on macOS for supported players). Right-click switches
focus to the playing app.

## Keys

| Key              | Does                                    |
| ---------------- | --------------------------------------- |
| Ctrl+Space       | Play or pause (anywhere in the bar)     |
| Ctrl+Shift+← / → | Previous or next track (empty bar only) |
| Scroll wheel     | Volume over the strip                   |

## Settings

No dedicated section. Control depends on the playing app and OS media APIs.
