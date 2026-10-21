<script setup lang="ts">
  import RowIcon from './RowIcon.vue';
  import { vFit } from '../fit';
  import { dragGesture } from './dragOut';
  import { rowKey, type Group } from './rows';
  import type { Suggestion } from '../types';

  const props = defineProps<{
    groups: Group[];
    selected: number;
    /** Destination names by id, for the monogram on rows that search a destination. */
    names?: Record<string, string>;
    /** While the run-a-row modifier is held: the keys for the first nine rows. */
    numbered?: string;
  }>();
  const emit = defineEmits<{
    choose: [index: number];
    hover: [index: number];
    /** A press on a row moved far enough to be a drag (the bar decides if it can be). */
    drag: [index: number];
    /** How many rows fit and are shown; the rest are left out, never scrolled to. */
    fit: [count: number];
  }>();

  const gesture = dragGesture((index) => emit('drag', index));

  // A row that searches a destination wears that destination's letter and color. Keyed on
  // the typed text instead, its color changed with every keypress.
  function iconLabel(row: Suggestion): string {
    const id = row.icon?.startsWith('dest:') ? row.icon.slice(5) : '';
    return (id && (props.names?.[id] ?? id)) || row.label;
  }

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
  <ul
    id="results"
    class="results sectioned"
    role="listbox"
    aria-label="Results"
    v-fit="(count: number) => emit('fit', count)"
  >
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
          @mousedown.prevent="gesture.down($event, group.start + offset)"
          @mouseup="gesture.up()"
          @click="onClick(group.start + offset, row)"
          @mousemove="
            gesture.move($event);
            onMove($event, group.start + offset, row);
          "
        >
          <RowIcon :icon="row.icon" :label="iconLabel(row)" :size="20" />
          <span class="item-text">
            <span class="label">{{ row.label }}</span>
            <span v-if="row.subtitle" class="item-sub">{{ row.subtitle }}</span>
          </span>
          <span class="item-acc">
            <kbd
              v-if="numbered && group.start + offset < 9"
              class="kbd row-number"
              aria-hidden="true"
              >{{ numbered }}{{ group.start + offset + 1 }}</kbd
            >
            <template v-else>
              <span v-if="row.icons?.length" class="row-icons" aria-hidden="true">
                <RowIcon v-for="icon in row.icons" :key="icon" :icon="icon" label="" :size="16" />
              </span>
              <kbd v-for="key in row.keys" :key="key" class="kbd" aria-hidden="true">{{ key }}</kbd>
              <template v-if="!row.keys?.length">{{ row.hint }}</template>
            </template>
          </span>
        </li>
      </ul>
    </li>
  </ul>
</template>
