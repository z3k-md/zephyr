---
title: Sync
summary: Google sign-in, encryption keys, and device approval.
status: partial
group: account
platforms: [windows, macos]
since: 2.5.0
code:
  - src-tauri/src/sync/**
  - src-tauri/src/commands/sync.rs
  - src/settings/SyncSection.vue
  - src/bar/DeviceCard.vue
  - src/bar/useDevices.ts
  - supabase/**
gaps:
  - Notes sync
  - Clipboard sync
---

Settings > Sync signs in with Google in the browser, creates end-to-end encryption keys and a
recovery key, and approves new computers from one you already use. Nothing syncs yet.

When another computer asks to join, a card with a short code appears in the bar. Allow or deny it
there or in Settings > Sync.

## Keys

| Key         | Does                              |
| ----------- | --------------------------------- |
| Ctrl+Y (⌘Y) | Allow a pending device (bar card) |
| Ctrl+N (⌘N) | Deny a pending device (bar card)  |

## Settings

Settings > Sync: sign in and out, the recovery key, the device list, and revoking a device.

## Privacy

The account key exists in the clear only on your computers. The server stores device public keys,
metadata, and the account key encrypted to each approved device; never the account key or the
recovery key in the clear. Sync requests run in Rust, so the webview never sees an access token.
