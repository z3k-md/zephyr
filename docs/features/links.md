---
title: Links
summary: zephyr:// URLs to type into the bar or search the web from other apps.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.4.0
code:
  - src-tauri/src/deeplink.rs
---

Other apps and scripts can drive Zephyr with links: `zephyr://open?q=text` shows the bar with text typed in, and `zephyr://search?q=crispr` searches the web for it (or opens it when it is a web address). Links never launch apps, open files or reach the shell.

## Use it

Register the `zephyr://` scheme when you install Zephyr. On macOS the link scheme comes from the installed app.

A search link only opens http(s) pages. An open link only stages text in the bar for you to confirm; it cannot run shell commands or open settings on its own.

## Keys

| Key | Does                                                             |
| --- | ---------------------------------------------------------------- |
| —   | Links are opened from outside the bar; then use the bar as usual |

## Settings

Links need no Settings changes.
