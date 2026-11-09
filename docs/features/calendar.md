---
title: Calendar
summary: Today's and tomorrow's meetings from your iCal feeds.
status: shipped
group: utilities
platforms: [windows, macos]
code:
  - src-tauri/src/calendar.rs
  - src-tauri/src/commands/calendar.rs
  - src/bar/calendar.ts
  - src/bar/calendar.test.ts
  - src/bar/useCalendar.ts
  - src/settings/CalendarSection.vue
---

`agenda` or `cal today` lists today; `cal tomorrow` lists tomorrow. A bare `cal` or `calendar`
lists today under other results so Calculator and the Calendar app stay on top. Enter joins a
meeting or copies details when there is no link. A meeting starting within 10 minutes (or one that
started less than 5 minutes ago) can lead the empty bar; the footer names the next one within 15
minutes. Add feeds in Settings > Calendar. With a join URL, Enter opens it; otherwise Enter copies
the event text.

## Keys

| Key   | Does               |
| ----- | ------------------ |
| ↑ / ↓ | Move the selection |
| Enter | Join or copy       |

## Settings

Settings > Calendar adds named iCal feeds: Google Calendar's secret address in iCal format,
Outlook's published ICS link, or iCloud's public link. The URL is stored in the system keychain
and is not shown again. Rename, reorder, and remove feeds there.

## Privacy

Each feed URL is a secret link kept in the keychain (Credential Manager on Windows). Names,
colors, and order stay in your settings file.
