<script setup lang="ts">
  import { computed } from 'vue';
  import RowIcon from './RowIcon.vue';
  import Keys from '../components/Keys.vue';
  import KeyHints from '../components/KeyHints.vue';
  import { vFit } from '../fit';
  import { dragGesture } from './dragOut';
  import { rowKey, type Group } from './rows';
  import type { Suggestion } from '../types';

  const props = defineProps<{
    groups: Group[];
    selected: number;
    /** While the run-a-row modifier is held: that modifier ("alt", "cmd"), which with a
     *  digit runs one of the first nine rows. */
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

  // Groups keyed by their title and how many came before with it, so a section keeps its
  // element as rows above it come and go (its start index alone would shift).
  const keyed = computed(() => {
    const seen = new Map<string, number>();
    return props.groups.map((group) => {
      const count = seen.get(group.title) ?? 0;
      seen.set(group.title, count + 1);
      return { ...group, key: `${group.title}#${count}` };
    });
  });

  // A card (the network card, a port's process) renders in the bar (#card), as the
  // list's first row.
  defineSlots<{ card(props: { row: Suggestion; selected: boolean }): unknown }>();

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
    <li v-for="group in keyed" :key="group.key" role="presentation">
      <div v-if="group.title" :id="`head-${group.start}`" class="section-head">
        {{ group.title }}
      </div>
      <ul role="group" :aria-labelledby="group.title ? `head-${group.start}` : undefined">
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
            'is-card': row.kind === 'netCard' || row.kind === 'portCard',
          }"
          @mousedown.prevent="gesture.down($event, group.start + offset)"
          @mouseup="gesture.up()"
          @click="onClick(group.start + offset, row)"
          @mousemove="
            gesture.move($event);
            onMove($event, group.start + offset, row);
          "
        >
          <slot
            v-if="row.kind === 'netCard' || row.kind === 'portCard'"
            name="card"
            :row="row"
            :selected="group.start + offset === selected"
          />
          <template v-else>
            <RowIcon :icon="row.icon" :label="row.label" :size="20" />
            <span class="item-text">
              <span class="label">{{ row.label }}</span>
              <span v-if="row.subtitle" class="item-sub">{{ row.subtitle }}</span>
            </span>
            <span class="item-acc">
              <Keys
                v-if="numbered && group.start + offset < 9"
                class="row-number"
                :keys="`${numbered}+${group.start + offset + 1}`"
                aria-hidden="true"
              />
              <template v-else>
                <span v-if="row.icons?.length" class="row-icons" aria-hidden="true">
                  <RowIcon v-for="icon in row.icons" :key="icon" :icon="icon" label="" :size="16" />
                </span>
                <!-- A row's hotkey shows only on the row in hand, so the list stays quiet. -->
                <Keys v-if="row.keys?.length" class="row-hotkey" :keys="row.keys" />
                <KeyHints v-else-if="row.keyHints?.length" :hints="row.keyHints" />
                <template v-else>{{ row.hint }}</template>
              </template>
            </span>
          </template>
        </li>
      </ul>
    </li>
  </ul>
</template>
