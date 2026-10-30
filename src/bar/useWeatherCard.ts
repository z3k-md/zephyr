// The weather card: "weather", "forecast", or either with a city ("weather in tokyo",
// "raleigh nc weather"). Here comes from the footer's forecast; a city is looked up as it's
// typed.

import { computed, nextTick, onUnmounted, ref, watch, type Ref } from 'vue';
import { errorMessage, openUrl, weatherAt } from '../api';
import type { Forecast } from '../types';
import { googleUrl } from './google';
import { placeName, useWeather, weatherCityFor } from './weather';

export function useWeatherCard(ctx: {
  query: Ref<string>;
  /** The card shows on the main bar with no scope, and gives way to the ⌘K menu. */
  shown: () => boolean;
  menuOpen: Ref<boolean>;
  inputEl: Ref<HTMLInputElement | null>;
  notice: Ref<string | null>;
  clearAfterOpen: () => void;
}) {
  const { query, menuOpen, inputEl, notice } = ctx;
  const forecast = useWeather();
  const weatherCity = computed(() => {
    return ctx.shown() ? weatherCityFor(query.value) : null;
  });
  const typedForecast = ref<{ city: string; forecast: Forecast | null } | null>(null);
  let weatherTimer = 0;
  watch(weatherCity, (city) => {
    window.clearTimeout(weatherTimer);
    if (!city || typedForecast.value?.city === city) return;
    weatherTimer = window.setTimeout(async () => {
      const found = await weatherAt(city).catch(() => null);
      if (weatherCity.value === city) typedForecast.value = { city, forecast: found };
    }, 350);
  });
  onUnmounted(() => window.clearTimeout(weatherTimer));

  const cardForecast = computed(() => {
    const city = weatherCity.value;
    if (city === null || menuOpen.value) return null;
    if (!city) return forecast.value;
    return typedForecast.value?.city === city ? typedForecast.value.forecast : null;
  });

  /** The hour picked on the card, from the next 48; null is now. A new place starts at now. */
  const cardHour = ref<number | null>(null);
  watch(weatherCity, () => (cardHour.value = null));

  /** Tab on the card: the next hour (Shift+Tab, the one before, and back to now). */
  function stepHour(by: 1 | -1) {
    const count = cardForecast.value?.hourly.length ?? 0;
    if (!count) return;
    const next = (cardHour.value ?? -1) + by;
    cardHour.value = next < 0 ? null : Math.min(next, count - 1);
  }

  function pickHour(hour: number) {
    cardHour.value = hour;
  }

  /** Ctrl+Enter on the card: a full forecast. */
  function openForecast() {
    const shown = cardForecast.value;
    if (!shown) return;
    void openUrl(googleUrl(`weather ${placeName(shown)}`))
      .then(ctx.clearAfterOpen)
      .catch((error) => (notice.value = errorMessage(error)));
  }

  /** The footer's weather: the weather card. */
  function showWeather() {
    if (!forecast.value) return;
    query.value = 'weather';
    // The caret at the end, not the word selected, so typing a city just carries on.
    void nextTick(() => {
      const input = inputEl.value;
      input?.focus();
      input?.setSelectionRange(input.value.length, input.value.length);
    });
  }

  return { forecast, cardForecast, cardHour, stepHour, pickHour, openForecast, showWeather };
}
