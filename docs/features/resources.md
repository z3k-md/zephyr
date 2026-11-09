---
title: System resources
summary: Live processes with fifteen-minute CPU, memory, GPU, and network.
status: shipped
group: utilities
platforms: [windows, macos]
since: 2.8.0
code:
  - src-tauri/src/resources.rs
  - src-tauri/src/gpu.rs
  - src-tauri/src/cputemp.rs
  - src-tauri/src/temps.rs
  - src-tauri/src/stats.rs
  - src-tauri/src/commands/resources.rs
  - src/bar/Resource*.vue
  - src/bar/ResourcesView.vue
---

Type `top` or **System Resources** to open the live process list. Apps sit above background
processes. Sparklines show CPU, memory, GPU, and network for about the last 15 minutes. The
footer chip links here too.

## Keys

| Windows     | macOS       | Does                      |
| ----------- | ----------- | ------------------------- |
| Delete      | ⌘⌫          | Quit the selected process |
| Shift+Space | Shift+Space | Process info              |
| Enter       | Enter       | Switch to the app         |
| Tab         | Tab         | Change sort               |
| ↑ / ↓       | ↑ / ↓       | Move the selection        |
| Ctrl+K      | ⌘K          | Row actions               |

## Settings

No dedicated section. Ignored paths follow the same rules as file search where they overlap.
