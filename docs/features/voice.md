---
title: Voice
summary: Hold a shortcut and speak; words land in the bar for Enter or a spoken command.
status: shipped
group: bar
platforms: [windows, macos]
code:
  - src-tauri/src/voice/**
  - src-tauri/src/commands/voice.rs
  - src/bar/useVoice.ts
  - src/bar/voice.ts
  - src/bar/voice.test.ts
---

Turn it on in Settings > Voice, which downloads the speech model (about 750 MB). Then hold the voice shortcut and speak; letting go leaves the words in the bar for Enter. Saying "search", "ask", "open" or "cancel" first or last runs it on release, and a scope word first, like "file" or "note", narrows it.

## Use it

The hotkey works only once the model is installed.

Spoken scope words (`app`, `files`, `bookmark`, `setting`, `clipboard`, `note`, `emoji`, `shell`) become the same `!` prefixes as typing. Trailing words like "search" or "open" run on release; "go" and "enter" are recognized only at the end.

## Keys

| Windows        | macOS               | Does                            |
| -------------- | ------------------- | ------------------------------- |
| Ctrl+Alt+Space | Control+Shift+Space | Hold to talk; release to finish |

While holding the shortcut, the bar shows the transcription. Release to fill the query or run a trailing command.

## Settings

**Settings > Voice** — enable voice input, record the hold shortcut, download or remove the model.

## Privacy

The mic is on only while the shortcut is held. Speech is recognized on this computer, and audio is never saved or sent anywhere.
