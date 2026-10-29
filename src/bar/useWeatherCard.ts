// The weather card: "weather", "forecast", or either with a city ("weather in tokyo").
// Here comes from the Today line's forecast; a city is looked up as it's typed.

import { computed, nextTick, onUnmounted, ref, watch, type Ref } from 'vue';
import { errorMessage, openUrl, weatherAt } from '../api';
import type { Forecast } from '../types';
import { googleUrl } from './google';
import { placeName, useWeather } from './weather';

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
    if (!ctx.shown()) return null;
    const asked = /^(?:weather|forecast)(?:\s+(?:in\s+|for\s+)?(.*))?$/i.exec(query.value.trim());
    return asked ? (asked[1] ?? '').trim() : null;
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

  return { forecast, cardForecast, openForecast, showWeather };
}
