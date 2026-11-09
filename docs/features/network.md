---
title: Network
summary: Router, internet, and DNS latency over time, and what's wrong right now.
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
router, the internet, and DNS, plus packet loss. The footer warns when something looks wrong. You
can also start a [speed test](/features/speed-test) from here.

## Use it

Live charts update every second while the view is open. Hover or focus a bucket to read exact
numbers. Ctrl+K runs actions such as starting or canceling a speed test.

## Keys

| Key         | Does            |
| ----------- | --------------- |
| Esc         | Back to the bar |
| Ctrl+K (⌘K) | Actions         |

While a speed test runs, Esc cancels it.

## Settings

Settings > Network turns **Watch connection** on or off (about 1 MB per hour). The same section
can run a speed test and shows the last result.
