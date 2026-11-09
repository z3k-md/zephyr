---
title: Weather
summary: Forecast from Open-Meteo for your place or any city you name.
status: shipped
group: utilities
platforms: [windows, macos]
since: 2.9.0
code:
  - src-tauri/src/weather.rs
  - src-tauri/src/commands/weather.rs
  - src/bar/Weather*.vue
  - src/bar/FooterWeather.vue
  - src/bar/weather.ts
  - src/bar/weather.test.ts
  - src/bar/useWeatherCard.ts
  - src/settings/WeatherSection.vue
---

Type `weather`, `weather raleigh nc`, or `stockholm sweden weather` to open the weather card.
The footer chip shows the current place when forecast data is available.

## Use it

The card shows now, the next hours, and a few days ahead. Tab steps through the next 48 hours with
each hour's chance of rain; Shift+Tab moves back. Ctrl+Enter opens a fuller forecast in the browser.

## Keys

| Key                 | Does               |
| ------------------- | ------------------ |
| Tab                 | Next hour          |
| Shift+Tab           | Previous hour      |
| Ctrl+Enter (⌘Enter) | Open full forecast |

## Settings

Settings > Weather picks a city, **Automatic** (by IP), and Fahrenheit or Celsius.
