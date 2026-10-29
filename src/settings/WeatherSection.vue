<script setup lang="ts">
  import { computed, ref, watch } from 'vue';
  import {
    errorMessage,
    saveWeatherByIp,
    saveWeatherPlace,
    saveWeatherUnit,
    weatherPlaces,
    weatherWarm,
  } from '../api';
  import InfoTip from '../components/InfoTip.vue';
  import { pickedUnit, regionUnit, useWeather } from '../bar/weather';
  import type { Place, Snapshot, WeatherUnit } from '../types';

  // Settings › Weather: where it's for, a city found as it's typed or automatic (where the
  // internet connection says the computer is), and Fahrenheit or Celsius.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  const UNITS: { id: WeatherUnit; label: string }[] = [
    { id: 'f', label: '°F' },
    { id: 'c', label: '°C' },
  ];

  function label(place: Place): string {
    return [place.name, place.region, place.country].filter(Boolean).join(', ');
  }

  const forecast = useWeather();
  const chosen = computed(() => props.snapshot.weatherPlace);
  /** With no city picked, where the last forecast was for. */
  const automatic = computed(() => {
    if (!props.snapshot.weatherByIp) return 'Search for a city';
    return !chosen.value && forecast.value?.place.name
      ? `Automatic · ${label(forecast.value.place)}`
      : 'Automatic';
  });
  const unit = computed(() => pickedUnit.value ?? regionUnit);

  const text = ref(chosen.value ? label(chosen.value) : '');
  watch(chosen, (place) => (text.value = place ? label(place) : ''));

  const found = ref<Place[]>([]);
  const highlighted = ref(0);
  const open = ref(false);
  const busy = ref(false);
  const error = ref<string | null>(null);
  let timer = 0;
  let asked = 0;
  /** Matches by text, so going back to a text shows them at once. */
  const seen = new Map<string, Place[]>();

  function show(places: Place[]) {
    found.value = places;
    highlighted.value = 0;
    open.value = places.length > 0;
  }

  function onInput() {
    window.clearTimeout(timer);
    const typed = text.value.trim().toLowerCase();
    if (typed.length < 2) {
      found.value = [];
      open.value = false;
      return;
    }
    const known = seen.get(typed);
    if (known) {
      asked++;
      show(known);
      return;
    }
    // A short pause, so a word typed quickly is one search rather than one a letter.
    timer = window.setTimeout(async () => {
      const ask = ++asked;
      const places = await weatherPlaces(typed).catch(() => null);
      if (places) seen.set(typed, places);
      if (ask === asked) show(places ?? []);
    }, 120);
  }

  async function save(request: Promise<Snapshot>) {
    busy.value = true;
    try {
      emit('apply', await request);
      error.value = null;
    } catch (err) {
      error.value = errorMessage(err);
    } finally {
      busy.value = false;
    }
  }

  function choose(place: Place | null) {
    open.value = false;
    found.value = [];
    void save(saveWeatherPlace(place));
  }

  function onKey(event: KeyboardEvent) {
    if (!open.value) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const count = found.value.length;
      highlighted.value =
        (highlighted.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
    } else if (event.key === 'Enter') {
      event.preventDefault();
      choose(found.value[highlighted.value] ?? null);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      open.value = false;
    }
  }

  /** Leaving the field without picking puts back what's saved. */
  function onBlur() {
    open.value = false;
    text.value = chosen.value ? label(chosen.value) : '';
  }
</script>

<template>
  <section id="section-weather">
    <h2>Weather</h2>
    <div class="field" data-focus="weather">
      <span class="field-label">
        Location
        <InfoTip
          text="Automatic follows where your internet connection says you are. Search to pin a city."
        />
      </span>
      <span class="place-field">
        <input
          v-model="text"
          type="text"
          :placeholder="automatic"
          spellcheck="false"
          autocomplete="off"
          role="combobox"
          aria-label="Search for a city"
          :aria-expanded="open"
          @focus="weatherWarm().catch(() => undefined)"
          @input="onInput"
          @keydown="onKey"
          @blur="onBlur"
        />
        <button
          v-if="chosen"
          type="button"
          class="text-button"
          :disabled="busy"
          @click="choose(null)"
        >
          {{ snapshot.weatherByIp ? 'Use my location' : 'Clear' }}
        </button>
        <ul v-if="open" class="select-menu place-list" role="listbox">
          <li
            v-for="(place, index) in found"
            :key="`${place.latitude},${place.longitude}`"
            role="option"
            :aria-selected="index === highlighted"
            class="select-option"
            :class="{ active: index === highlighted }"
            @mousedown.prevent="choose(place)"
            @mousemove="highlighted = index"
          >
            {{ place.name }}
            <small>{{ [place.region, place.country].filter(Boolean).join(', ') }}</small>
          </li>
        </ul>
      </span>
    </div>
    <label class="check" data-focus="locate">
      <input
        type="checkbox"
        :checked="snapshot.weatherByIp"
        :disabled="busy"
        @change="save(saveWeatherByIp(!snapshot.weatherByIp))"
      />
      Locate by IP address
      <InfoTip
        text="With no city picked, asks ipapi.co once a day where your internet connection is. Off, the weather shows only for a city you pick."
      />
    </label>
    <div class="field" data-focus="units">
      <span class="field-label">
        Units
        <InfoTip text="Temperatures, and wind in mph or km/h." />
      </span>
      <div class="segments" role="radiogroup" aria-label="Units">
        <button
          v-for="option in UNITS"
          :key="option.id"
          type="button"
          role="radio"
          class="segment"
          :class="{ chosen: unit === option.id }"
          :aria-checked="unit === option.id"
          :disabled="busy"
          @click="save(saveWeatherUnit(option.id))"
        >
          {{ option.label }}
        </button>
      </div>
    </div>
    <p v-if="error" class="notice">{{ error }}</p>
  </section>
</template>
