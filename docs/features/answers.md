---
title: Answers
summary: Math, units, time, dates and network facts in the top row; Enter copies the result.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.4.0
code:
  - src-tauri/src/answer.rs
  - src/bar/answerHistory.ts
  - src-tauri/src/net/ip.rs
  - src-tauri/src/net/ports.rs
  - src/bar/PortCard.vue
---

Calculations, conversions, clock questions and network lookups answer in the top row as you type. Enter copies the answer. The last few you copied show under a new one.

## Use it

- Math and units: `12 * 7`, `5 km to miles`
- Time zones: `time in tokyo`, `3pm pst to cet`
- Dates and timestamps: `unix`, `1760000000 unix`, `week`, `day of year`, `now iso`
- Network: `ip` shows local and public addresses; `port 3000` or `:3000` shows what's listening there and can quit it; `ports` lists every listening port

When a port card is showing, ↑ ↓ move between processes on that port. Enter on the top row opens Resources for that process, or quits it after a confirmation.

## Keys

| Key        | Does                                                 |
| ---------- | ---------------------------------------------------- |
| Enter      | Copy the answer row, or act on the selected port row |
| ↑ ↓        | On a port question, move between matching processes  |
| Ctrl+Enter | With the IP card up, open the Network utility        |

## Settings

Answers need no Settings changes.
