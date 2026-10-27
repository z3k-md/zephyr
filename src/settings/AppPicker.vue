<script setup lang="ts">
  import { computed, ref } from 'vue';
  import { searchApps } from '../api';
  import RowIcon from '../bar/RowIcon.vue';

  // A search field for installed apps, matched and ranked as the bar does, with their icons.
  // ↑/↓ and Enter or a click picks one; apps in `exclude` aren't offered.
  const props = defineProps<{ exclude: string[]; placeholder?: string }>();
  const emit = defineEmits<{ pick: [id: string, name: string] }>();

  const text = ref('');
  const found = ref<[string, string][]>([]);
  const highlighted = ref(0);
  const open = ref(false);
  let asked = 0;

  const offered = computed(() =>
    found.value.filter(([id]) => !props.exclude.includes(id)).slice(0, 6)
  );

  async function search() {
    const ask = ++asked;
    const apps = await searchApps(text.value, 12).catch(() => []);
    if (ask !== asked) return;
    found.value = apps;
    highlighted.value = 0;
    open.value = true;
  }

  function pick(app: [string, string] | undefined) {
    if (!app) return;
    emit('pick', app[0], app[1]);
    text.value = '';
    void search();
  }

  function onKey(event: KeyboardEvent) {
    const count = offered.value.length;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open.value) return void search();
      if (count) {
        highlighted.value =
          (highlighted.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
      }
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (open.value) pick(offered.value[highlighted.value]);
    } else if (event.key === 'Escape' && open.value) {
      event.preventDefault();
      event.stopPropagation();
      open.value = false;
    }
  }
</script>

<template>
  <div class="app-picker">
    <span class="app-picker-field">
      <svg class="app-picker-mark" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="11" cy="11" r="6.5" />
        <path d="M16 16.5 20 20.5" />
      </svg>
      <input
        v-model="text"
        type="text"
        :placeholder="placeholder ?? 'Add an app'"
        spellcheck="false"
        autocomplete="off"
        role="combobox"
        :aria-expanded="open && offered.length > 0"
        @input="search"
        @focus="search"
        @keydown="onKey"
        @blur="open = false"
      />
    </span>
    <ul v-if="open && offered.length" class="select-menu app-picker-list" role="listbox">
      <li
        v-for="(app, index) in offered"
        :key="app[0]"
        role="option"
        class="select-option"
        :aria-selected="index === highlighted"
        :class="{ active: index === highlighted }"
        @mousedown.prevent="pick(app)"
        @mousemove="highlighted = index"
      >
        <RowIcon :icon="`app:${app[0]}`" :label="app[1]" :size="20" />
        {{ app[1] }}
      </li>
    </ul>
  </div>
</template>
