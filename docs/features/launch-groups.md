---
title: Launch groups
summary: Open several apps, links and folders at once by name or hotkey.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.8.0
code:
  - src-tauri/src/launch_groups.rs
  - src/settings/LaunchGroupsSection.vue
  - src/settings/AppPicker.vue
---

Launch groups open several apps, links and folders in one go ("Coding", "Gaming"). Apps already running come forward. Run a group by typing its name in the bar or pressing its global hotkey.

## Use it

Create groups in Settings > Launch groups. Each group has a name, an optional hotkey, and a list of apps (from the same search ranking as the bar), links and folders. An empty group is dropped when you leave the settings page.

## Keys

| Key    | Does                                     |
| ------ | ---------------------------------------- |
| Enter  | Run the launch group row you selected    |
| Delete | In Settings, remove an item from a group |

## Settings

**Settings > Launch groups** — add and edit groups, pick apps, set a hotkey per group.
