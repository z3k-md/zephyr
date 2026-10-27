<script setup lang="ts">
  import { computed, ref, watch } from 'vue';
  import { errorMessage, saveWeatherPlace, weatherPlaces } from '../api';
  import type { Place, Snapshot } from '../types';

  // Where the weather is for: a city found as it's typed, or left empty for where the IP
  // address says the computer is.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  function label(place: Place): string {
    return [place.name, place.region, place.country].filter(Boolean).join(', ');
  }

  const chosen = computed(() => props.snapshot.weatherPlace);
  const text = ref(chosen.value ? label(chosen.value) : '');
  watch(chosen, (place) => (text.value = place ? label(place) : ''));

  const found = ref<Place[]>([]);
  const highlighted = ref(0);
  const open = ref(false);
  const error = ref<string | null>(null);
  let timer = 0;
  let asked = 0;

  function onInput() {
    window.clearTimeout(timer);
    const typed = text.value.trim();
    if (typed.length < 2) {
      found.value = [];
      open.value = false;
      return;
    }
    timer = window.setTimeout(async () => {
      const ask = ++asked;
      const places = await weatherPlaces(typed).catch(() => []);
      if (ask !== asked) return;
      found.value = places;
      highlighted.value = 0;
      open.value = places.length > 0;
    }, 250);
  }

  async function choose(place: Place | null) {
    open.value = false;
    found.value = [];
    try {
      emit('apply', await saveWeatherPlace(place));
      error.value = null;
    } catch (err) {
      error.value = errorMessage(err);
    }
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
      void choose(found.value[highlighted.value] ?? null);
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
  <div class="field" data-focus="weather">
    <span>Weather for</span>
    <span class="place-field">
      <input
        v-model="text"
        type="text"
        placeholder="Automatic, from your IP address"
        spellcheck="false"
        autocomplete="off"
        role="combobox"
        :aria-expanded="open"
        @input="onInput"
        @keydown="onKey"
        @blur="onBlur"
      />
      <button
        v-if="chosen"
        type="button"
        class="text-button"
        title="Use the IP address again"
        @click="choose(null)"
      >
        Automatic
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
    <p v-if="error" class="notice">{{ error }}</p>
  </div>
</template>
