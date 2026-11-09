---
title: Color picker
summary: Freeze the screen and copy a pixel's color under the cursor.
status: shipped
group: utilities
platforms: [windows, macos]
code:
  - src-tauri/src/picker.rs
  - src-tauri/src/capture.rs
  - src-tauri/src/commands/picker.rs
  - src-tauri/src/commands/capture.rs
  - src-tauri/src/permissions.rs
  - src/picker/**
  - src/settings/PermissionsSection.vue
---

Type `pick color`, `color picker`, or `eyedropper` in the bar, or set a hotkey in Settings >
Shortcuts. The screen freezes and a loupe magnifies the pixel under the cursor. Move the pointer
or nudge with arrow keys (Shift for 10 pixels). The hex updates in the loupe. Parsed color strings
in the bar still use [Colors](/features/colors). On macOS the picker reads the main display and
needs Screen Recording permission.

## Keys

| Key                | Does                        |
| ------------------ | --------------------------- |
| ↑ / ↓ / ← / →      | Move one pixel (Shift: ten) |
| Enter or click     | Copy hex                    |
| Esc or right-click | Cancel                      |

## Settings

Settings > Permissions (macOS) covers Screen Recording for the picker. Settings > Shortcuts can
assign a global hotkey.
