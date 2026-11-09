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

Installed copies check for updates shortly after launch and whenever the bar opens (at most every few minutes), and install them automatically. Settings and the tray menu also have **Check for updates**. A card in the corner shows the download, then counts down a few seconds before installing; **Install now** skips the wait, and **Later** puts it off, with a button in the bar to install whenever suits.

## Use it

Updates download in the background and verify against Zephyr's updater signing key. The Windows installer itself is not Authenticode-signed yet; SmartScreen may warn on first install, while updates still use Zephyr's own signature.

Choose **Stable** or **Nightly** in Settings; nightly builds come from `develop` and stay until a newer stable passes them.

## Keys

| Key | Does                                         |
| --- | -------------------------------------------- |
| —   | Updates run from the corner card or Settings |

## Settings

**Settings > General** — **Check for updates** and the update channel (Stable or Nightly).

## How it works

The updater reads `latest.json` from GitHub Releases (or the nightly manifest when that channel is on). Deferred installs download again when you choose to install later, and the signature is checked again each time.
