---
title: Network
summary: Router, internet, and DNS latency over time plus current faults.
status: shipped
group: utilities
platforms: [windows, macos]
since: 2.10.0
code:
  - src-tauri/src/net/dns.rs
  - src-tauri/src/net/metrics.rs
  - src-tauri/src/net/mod.rs
  - src-tauri/src/net/platform.rs
  - src-tauri/src/commands/net.rs
  - src/bar/Network*.vue
  - src/bar/network.ts
---

Type `ping` or **Network** to open the connection view. Charts show median and p95 latency to your
router, the internet, and DNS, plus packet loss. The footer warns when something looks wrong.
Start a [speed test](/features/speed-test) from here. Charts update every second while the view
is open; hover or focus a bucket for exact numbers.

## Keys

| Key         | Does            |
| ----------- | --------------- |
| Esc         | Back to the bar |
| Ctrl+K (⌘K) | Actions         |

While a speed test runs, Esc cancels it.

## Settings

Settings > Network turns **Watch connection** on or off (about 1 MB per hour). The same section
can run a speed test and shows the last result.
