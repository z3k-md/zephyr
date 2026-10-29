<script setup lang="ts" generic="A extends MenuAction">
  import Keys from './Keys.vue';
  import { vFit } from '../fit';
  import type { MenuAction } from './actionMenu';

  // The ⌘K menu's list (see actionMenu.ts). Each row's id is `${id}-${index}`, for the
  // search box's aria-activedescendant.
  defineProps<{ id: string; actions: A[]; index: number }>();
  const emit = defineEmits<{ run: [action: A]; hover: [index: number]; fit: [count: number] }>();
</script>

<template>
  <ul
    :id="id"
    v-fit="(count: number) => emit('fit', count)"
    class="results actions"
    role="listbox"
    aria-label="Actions"
  >
    <li
      v-for="(action, at) in actions"
      :id="`${id}-${at}`"
      :key="action.label"
      role="option"
      :aria-selected="at === index"
      :aria-disabled="action.disabled ? true : undefined"
      class="item"
      :class="{ selected: at === index }"
      @mousedown.prevent
      @click="emit('run', action)"
      @mousemove="emit('hover', at)"
    >
      <span class="label" :class="{ off: action.disabled }">{{ action.label }}</span>
      <span v-if="action.disabled" class="action-why">{{ action.disabled }}</span>
      <Keys v-else-if="action.keys" :keys="action.keys" />
    </li>
  </ul>
</template>
