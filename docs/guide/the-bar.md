---
title: The bar
summary: Open the launcher, type or pick a row, and move it around the screen.
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

`Alt+Space` opens the bar (`Command+Space` on a Mac keyboard). Typing the start of an app's name (`chr`, `excel`, or initials like `vsc`) selects that app, and Enter opens it; other free text is searched on Google, and a link (`example.com/docs`) opens directly. Picking the Google row for text that matched an app sends it to the web, and Zephyr remembers that choice for the same text next time. Shift+Enter on a search opens it in Google's AI Mode; on a browser, it opens a private window.

Narrowing a search with `!words`, the answers that show as you type, voice input, and apps and windows each have their own page: [Scopes](/features/scopes), [Answers](/features/answers), [Voice](/features/voice) and [Apps and windows](/features/apps-and-windows). Every utility, such as [clipboard history](/features/clipboard), is listed in the sidebar.

## Open it

`Alt+Space` on Windows and `Command+Space` on macOS toggles the bar. The same shortcut closes it when it is already open, like Esc.

## Type

Free text searches on Google unless it clearly matches an app, a link, or another built-in row. A faint completion after the cursor shows what Enter would act on; **Right** or **End** accepts it while the cursor sits at the end of the text.

Scoped searches (`!file`, `>`, and the rest) skip type-ahead.

## Row keys and actions

- **Alt+1..9** (⌘1..9) runs the Nth row; hold the key to see the numbers. On Windows, a bare **Alt** tap also shows the numbers.
- **Ctrl+K** (⌘K) lists more actions for the selected row: copy the link, text or path, show a file in its folder, quit an app, or give it a hotkey or alias.
- **Ctrl+S** (⌘S) opens Settings.
- **Esc** steps back to the main bar, or closes the bar when you are already on the main view.

## Move it

**Ctrl+drag** (⌘-drag) moves the bar. A snap grid appears over the screen while you drag; the bar reopens where you left it.

## The empty bar

With nothing typed, the bar shows recent searches, recent apps, the utilities you use most, and your IP address. Reopening the bar soon after closing it returns to the view you were in (**Settings > General**).

## The footer

The footer shows CPU, GPU and memory, network trouble, the weather and the next meeting; click any of them for more.
