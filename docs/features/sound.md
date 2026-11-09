---
title: Sound
summary: Per-app volume, mute, and output device on Windows.
status: shipped
group: utilities
platforms: [windows]
since: 2.11.0
code:
  - src-tauri/src/audio.rs
  - src-tauri/src/volume_keys.rs
  - src-tauri/src/keyhook.rs
  - src-tauri/src/commands/sound.rs
  - src/bar/Sound*.vue
  - src/bar/MixerRow.vue
  - src/bar/SpeakerIcon.vue
  - src/bar/mixer.ts
  - src/osd/**
---

Type `vol`, `mixer`, or **Sound** to open the volume mixer (Windows only). Every app's volume
has a live meter. ↑↓ picks a row, ←→ adjusts level (Shift for 10%), M mutes, 1–9 and 0 jump to
10–100%, and Tab switches the output device. With the bar open, hardware volume keys can show
levels in the bar instead of the overlay.

## Use it

The master output device is always the first row. Pick an app row and move levels with the arrow
keys or digit keys. Ctrl+K lists actions such as mute or switch device.

## Keys

| Key                 | Does                              |
| ------------------- | --------------------------------- |
| ↑ / ↓               | Pick a row                        |
| ← / →               | Lower or raise level (Shift: 10%) |
| M or Space or Enter | Mute or unmute the row            |
| 1–9                 | Set 10–90%                        |
| 0                   | Set 100%                          |
| Tab                 | Next output device                |
| Ctrl+K              | Actions                           |
| Esc                 | Back                              |

With **Volume keys show Zephyr's overlay** on, Alt with a volume key picks an app for the keys to
change until the overlay fades.

## Settings

Settings > Sound (Windows) replaces the Windows volume flyout with Zephyr's overlay and sets where
it appears. Per-app mixing needs Windows; macOS does not offer this utility in the bar.
