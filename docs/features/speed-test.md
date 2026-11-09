---
title: Speed test
summary: Download, upload, and lag under load from Cloudflare.
status: shipped
group: utilities
platforms: [windows, macos]
since: 2.10.0
code:
  - src-tauri/src/net/speed.rs
  - src/speedTest.ts
  - src/components/SpeedPanel.vue
  - src/components/speedPanel.css
---

Type **Run speed test** or **speed test** in the bar, or run it from [Network](/features/network)
or Settings > Network. The test takes about 15 seconds and measures download, upload, and how much
slower the connection reacts under load (bufferbloat).

## Use it

While it runs, live numbers update in the panel. Esc cancels. The last result is saved and shown
in Settings and on the Network view timeline.

## Keys

| Key | Does                 |
| --- | -------------------- |
| Esc | Cancel while running |

## Settings

Settings > Network runs the test and shows the latest result. **Watch connection** is separate.
