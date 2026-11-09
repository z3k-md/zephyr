---
title: Colors
summary: Parse color syntax into HEX, RGB, HSL and OKLCH with shades and contrast.
status: shipped
group: bar
platforms: [windows, macos]
code:
  - src/bar/ColorCard.vue
  - src/bar/color.ts
  - src/bar/color.test.ts
  - src/bar/useColorCard.ts
---

`#c026d3`, `rgb(192 38 211)`, `hsl(296 70% 49%)`, `oklch(0.55 0.2 330)`, or `color coral` show a color card with HEX, RGB, HSL and OKLCH, seven shades, and contrast with white and black. A bare name (`red`, `orange`) still finds an app instead of opening the card. When the text matches a color, the card replaces other top-row answers.

## Keys

| Key             | Does                                     |
| --------------- | ---------------------------------------- |
| ↑ ↓             | Pick a format (HEX, RGB, HSL, OKLCH)     |
| Tab / Shift+Tab | Step through shades                      |
| Enter           | Copy the chosen format and close the bar |
