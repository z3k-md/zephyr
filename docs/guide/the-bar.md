---
title: The bar
summary: Open it over any app, type, and run a row.
code:
  - src/bar/BarView.vue
  - src/bar/ResultList.vue
  - src/bar/RowIcon.vue
  - src/bar/rows.ts
  - src/bar/rows.test.ts
  - src/bar/selection.ts
  - src/bar/selection.test.ts
  - src/bar/useRunKey.ts
  - src/bar/useTypeAhead.ts
  - src/bar/queryField.ts
  - src/bar/queryField.test.ts
  - src/bar/useSuggest.ts
  - src/bar/google.ts
  - src/bar/footerStats.ts
  - src/bar/footerFit.ts
  - src/bar/footerFit.test.ts
  - src/grid/**
  - src-tauri/src/window.rs
  - src-tauri/src/suggest.rs
  - src-tauri/src/history.rs
  - src-tauri/src/commands/bar.rs
  - src-tauri/src/commands/search.rs
  - src-tauri/src/query.rs
  - src/components/ActionMenu.vue
  - src/components/actionMenu.ts
  - src/components/KeyHints.vue
  - src/onboarding/OnboardingView.vue
---

`Alt+Space` (`⌘Space` on macOS) opens and closes the bar.

## Type

- The start of an app's name (`chr`, `excel`) or its initials (`vsc`) selects the app. Enter opens it.
- A link (`example.com/docs`) opens directly.
- Other text searches Google. Picking the Google row for text that matched an app sends that text to the web from then on.
- Shift+Enter on a search opens it in Google's AI Mode. On a browser, it opens a private window.
- Faint type-ahead shows what Enter will run. → or End accepts it when the cursor is at the end. Scoped searches (`!file`, `>`) have none.

## Keys

| Key                | Does                                                                                   |
| ------------------ | -------------------------------------------------------------------------------------- |
| Enter              | Run the selected row                                                                   |
| Alt+1..9 (⌘1..9)   | Run the Nth row; hold to show the numbers                                              |
| Alt (Windows)      | Tap to show the row numbers                                                            |
| Ctrl+K (⌘K)        | More actions: copy the link, text or path, show in folder, quit, set a hotkey or alias |
| Ctrl+S (⌘S)        | Open Settings                                                                          |
| Esc                | Back to the main bar; on the main bar, close it                                        |
| Ctrl+drag (⌘-drag) | Move the bar; it snaps to a grid and reopens where you left it                         |

## Empty bar

Recent searches, recent apps, your most-used utilities and your IP address. Reopening the bar soon after closing it returns to the last view (Settings > General).

## Footer

CPU, GPU and memory, network trouble, the weather and the next meeting. Click one for details.

## More

[Scopes](/features/scopes) narrow a search with `!words`. [Answers](/features/answers) appear as you type. [Voice](/features/voice) fills the bar by speech. [Apps and windows](/features/apps-and-windows) covers running apps. Utilities, such as [clipboard history](/features/clipboard), are in the sidebar.
