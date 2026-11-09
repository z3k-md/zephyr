---
title: Shell
summary: Run shell commands in the bar, or hand off to a real terminal.
status: shipped
group: utilities
platforms: [windows, macos]
since: 2.5.0
code:
  - src-tauri/src/shell.rs
  - src-tauri/src/handoff.rs
  - src-tauri/src/commands/shell.rs
  - src/shell/**
---

`>` or `!sh` opens the shell in the bar. On Windows use Git Bash, PowerShell, or WSL; on macOS
commands run in your login shell.

## Use it

Type a command and Enter runs it; output streams in the bar. Recent commands appear while you
type. Ctrl+Enter (⌘Enter) opens the same command in a real terminal. A running command stops with
Ctrl+C (⌘C on macOS).

## Keys

| Key                 | Does                      |
| ------------------- | ------------------------- |
| Enter               | Run                       |
| Ctrl+Enter (⌘Enter) | Open in terminal          |
| Ctrl+C (⌘C)         | Stop a running command    |
| Tab                 | Pick from recent commands |
| Ctrl+K (⌘K)         | Actions                   |

## Settings

Settings > Terminal chooses which shell program runs on Windows (Git Bash, PowerShell, or WSL).
macOS always uses the login shell.
