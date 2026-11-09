---
title: Stocks and crypto
summary: Live quotes from Yahoo Finance in a card as you type a ticker or coin.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.11.0
code:
  - src-tauri/src/stocks.rs
  - src-tauri/src/commands/stocks.rs
  - src/bar/TickerCard.vue
  - src/bar/ticker.ts
  - src/bar/ticker.test.ts
  - src/bar/useTickerCard.ts
---

`TSLA`, `$aapl` or `aapl stock`, and `eth`, `btc` or `bitcoin` in any case show a price card with change and chart from Yahoo Finance. The card appears on the main bar when the text is clearly a ticker and nothing else is using the top row.

## Use it

Type a symbol on its own. The card refreshes about every thirty seconds while it stays on screen. Tab steps the chart through 1D, 5D, 1M and 1Y; a new symbol starts on 1D again.

## Keys

| Key                 | Does                                    |
| ------------------- | --------------------------------------- |
| Tab / Shift+Tab     | Step the chart span forward or back     |
| Ctrl+Enter (⌘Enter) | Open the full quote on Yahoo Finance    |
| Enter               | Stays on the search row (does not copy) |

## Settings

Stocks need no Settings changes.
