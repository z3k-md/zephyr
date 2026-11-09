---
title: Sync
summary: Sign in with Google, set up encryption keys, and approve new computers.
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
  - Sync notes across computers
  - Sync clipboard history across computers
---

Settings > Sync signs in with Google, creates end-to-end encryption keys and a recovery key, and
lets you approve new computers from one you already use. Syncing notes and clipboard history is
planned next; no user data syncs yet.

## Use it

Sign in with Google in the browser, save the recovery key, and finish setup on this computer. When
another device asks to join, a card appears in the bar with a short code; allow or deny it there
or in Settings > Sync.

## Keys

| Key         | Does                              |
| ----------- | --------------------------------- |
| Ctrl+Y (⌘Y) | Allow a pending device (bar card) |
| Ctrl+N (⌘N) | Deny a pending device (bar card)  |

## Settings

Settings > Sync covers sign-in, recovery key, device list, sign out, and revoking a device.

## Privacy

Notes and clipboard items are meant to sync sealed with one account key only devices ever see. The
server stores device public keys, metadata, and ciphertext envelopes—not the account key or your
recovery key in the clear. Sync requests run in Rust; the webview never sees an access token.
