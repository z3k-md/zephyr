---
title: Weather
summary: Open-Meteo forecast for your place or a named city.
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
The footer chip shows the current place when forecast data is available. The card shows now, the
next hours, and four days ahead.

## Keys

| Key                 | Does               |
| ------------------- | ------------------ |
| Tab                 | Next hour          |
| Shift+Tab           | Previous hour      |
| Ctrl+Enter (⌘Enter) | Open full forecast |

Tab steps through the next 48 hours; each step shows that hour's chance of rain.

## Settings

Settings > Weather picks a city, **Automatic** (by IP), and Fahrenheit or Celsius.
