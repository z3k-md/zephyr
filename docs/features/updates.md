---
title: Updates
summary: Automatic checks and signed installs from GitHub Releases.
status: shipped
group: bar
platforms: [windows, macos]
since: 1.3.0
code:
  - src-tauri/src/updater.rs
  - src-tauri/src/commands/updates.rs
  - src/update/**
gaps:
  - Authenticode signing for the Windows installer
---

Zephyr checks for updates 5 seconds after launch and when the bar opens, at most every 3 minutes, and installs them automatically. A card in the corner shows the download, then installs after a 5-second countdown. **Install now** skips the countdown; **Later** defers it to a button in the bar.

## Settings

Settings > General has **Check for updates** and the channel: **Stable** or **Nightly**. Nightly builds come from `develop`; a newer stable release replaces them. The tray menu also has **Check for updates**.

## How it works

The updater reads `latest.json` from GitHub Releases, or the nightly manifest on that channel. Every download is verified against Zephyr's updater signing key, including a deferred install, which downloads again. This signature is separate from Authenticode, which the Windows installer doesn't have yet.
