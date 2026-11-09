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

Type `vol`, `mixer`, or **Sound** to open the volume mixer (Windows only). The master output
device is always the first row. Every app has a live meter. With the bar open, hardware volume
keys can show levels in the bar instead of the overlay.

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
