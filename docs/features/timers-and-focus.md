---
title: Timers and focus
summary: Countdown timers and one focus session at a time, with optional break and site blocking.
status: shipped
group: utilities
platforms: [windows, macos]
since: 2.11.0
code:
  - src-tauri/src/timer.rs
  - src-tauri/src/focus.rs
  - src-tauri/src/countdown.rs
  - src-tauri/src/commands/countdown.rs
  - src/bar/timers.ts
  - src/bar/timers.test.ts
  - src/bar/useTimers.ts
  - src/bar/FocusStrip.vue
  - src/bar/focusBlock.test.ts
  - src/settings/FocusSection.vue
---

`timer 5m tea` counts down and notifies; bare `timer` offers 5, 10, and 15 minutes. Enter on a
running timer cancels it. `focus` offers 25, 50, and 90 minutes; `focus 50m`, `focus 1h deep work`,
or `focus deep work` starts a focus session. Only one focus session runs at a time; starting
another replaces it. The footer shows an active timer or focus session.

## Use it

Pick a preset row or type a duration and optional label. When a focus session ends, Settings can
start a break timer and minimize listed apps. Sites listed in Settings are blocked by the
[browser extension](/features/browser-extension) until the session ends.

## Keys

| Key                              | Does        |
| -------------------------------- | ----------- |
| Enter on a running timer         | Cancel      |
| Enter on a running focus session | End session |

## Settings

Settings > Focus sets the default focus length, break length (0 means no break), apps to minimize,
and sites to block. Blocking needs the browser extension; the session still runs without it.
