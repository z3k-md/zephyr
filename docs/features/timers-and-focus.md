---
title: Timers and focus
summary: Countdown timers, and focus sessions that block distracting sites.
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

`timer 5m tea` counts down and notifies. `timer` alone offers 5, 10 and 15 minutes.

`focus` offers 25, 50 and 90 minutes. `focus 50m`, `focus 1h deep work` or `focus deep work`
starts a session. One session runs at a time; starting another replaces it.

The footer shows the running timer or session. Enter on its row cancels it.

## Settings

Settings > Focus sets the default length, a break that starts as a timer when the session ends (0
for none), apps to minimize, and sites to block until it ends. Blocking needs the
[browser extension](/features/browser-extension); without it the session still runs.
