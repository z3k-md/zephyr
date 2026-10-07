<script setup lang="ts">
  import RowIcon from './RowIcon.vue';
  import { rowKey, type Group } from './rows';
  import type { Suggestion } from '../types';

  defineProps<{ groups: Group[]; selected: number }>();
  const emit = defineEmits<{ choose: [index: number]; hover: [index: number] }>();

  // WebKit sends a synthetic mousemove when the list scrolls under a still pointer; only a
  // real move selects, and a click only runs the row the pointer actually moved onto.
  let lastX = -1;
  let lastY = -1;
  let pointerKey = '';

  function onMove(event: MouseEvent, index: number, row: Suggestion) {
    if (event.screenX === lastX && event.screenY === lastY) return;
    lastX = event.screenX;
    lastY = event.screenY;
    pointerKey = rowKey(row);
    emit('hover', index);
  }

  function onClick(index: number, row: Suggestion) {
    if (rowKey(row) === pointerKey) emit('choose', index);
    else emit('hover', index);
  }
</script>

<template>
  <ul id="results" class="results sectioned" role="listbox" aria-label="Results">
    <li v-for="group in groups" :key="group.start" role="presentation">
      <div :id="`head-${group.start}`" class="section-head">{{ group.title }}</div>
      <ul role="group" :aria-labelledby="`head-${group.start}`">
        <li
          v-for="(row, offset) in group.rows"
          :id="`result-${group.start + offset}`"
          :key="rowKey(row)"
          role="option"
          :aria-selected="group.start + offset === selected"
          class="item"
          :class="{
            selected: group.start + offset === selected,
            'is-answer': row.kind === 'answer',
          }"
          @mousedown.prevent
          @click="onClick(group.start + offset, row)"
          @mousemove="onMove($event, group.start + offset, row)"
        >
          <RowIcon :icon="row.icon" :label="row.label" :size="20" />
          <span class="item-text">
            <span class="label">{{ row.label }}</span>
            <span v-if="row.subtitle" class="item-sub">{{ row.subtitle }}</span>
          </span>
          <span class="item-acc">
            <kbd v-for="key in row.keys" :key="key" class="kbd" aria-hidden="true">{{ key }}</kbd>
            <template v-if="!row.keys?.length">{{ row.hint }}</template>
          </span>
        </li>
      </ul>
    </li>
  </ul>
</template>
