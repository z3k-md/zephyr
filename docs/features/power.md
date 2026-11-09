---
title: Power
summary: Shutdown, restart, sleep, lock, sign out and scheduled power from the bar.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.6.0
code:
  - src-tauri/src/power.rs
  - src-tauri/src/commands/power.rs
  - src/bar/usePower.ts
---

Type `shutdown`, `restart`, `sleep`, `lock`, `sign out`, and on Windows `bios` (restart to UEFI). Shutting down, restarting and signing out need a second Enter. `shutdown in 30m` or `restart at 11pm` schedules one, and `cancel shutdown` stops it. Sleep and lock run immediately without a second Enter.

When the text matches a power command, a preview row appears at the top. Confirm destructive actions with Enter again, or press Esc to back out. Scheduled `bios` restarts to firmware are only offered for immediate use on Windows.

## Keys

| Key   | Does                            |
| ----- | ------------------------------- |
| Enter | Run or confirm the power action |
| Esc   | Cancel a pending confirmation   |
