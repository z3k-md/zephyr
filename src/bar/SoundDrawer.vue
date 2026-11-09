<script setup lang="ts">
  import { computed, ref, watch } from 'vue';
  import MixerRow from './MixerRow.vue';
  import { useMixer } from './mixer';
  import type { VolumeShown } from '../types';

  // The volume keys while the bar is open: a few levels slide up over the footer instead of
  // the overlay, the keys' target lit. Each can be dragged, scrolled or muted right here; the
  // bar keeps the drawer up while the pointer is on it, and hides it a moment after the last
  // press otherwise. Dragging the handle up, the "All apps" button or Tab grows it into the
  // Sound view.
  const props = defineProps<{ shown: VolumeShown }>();
  const emit = defineEmits<{ open: [string]; hold: [boolean] }>();

  /** Master, the target, and what's playing, up to this many rows. */
  const ROWS = 4;
  /** How far up the handle goes before the drawer grows into the Sound view. */
  const PULL_PX = 16;

  // The levels only refresh with a key press, so what's set here shows until the next one.
  const { held, withHeld, toggleMute, slide, wheel } = useMixer(() => {}, Infinity);
  watch(
    () => props.shown,
    () => (held.value = {})
  );

  const rows = computed(() => {
    const { key, levels } = props.shown;
    const picked = levels.filter(
      (level, index) => index === 0 || level.key === key || level.active
    );
    // The target always makes the cut.
    const kept = picked.slice(0, ROWS);
    const target = picked.find((level) => level.key === key);
    if (target && !kept.includes(target)) kept[ROWS - 1] = target;
    return withHeld(kept);
  });

  const appCount = computed(() => props.shown.levels.length - 1);

  // Hovered or mid-drag, the drawer stays.
  const hovered = ref(false);
  const dragging = ref(false);
  function setHovered(on: boolean) {
    hovered.value = on;
    emit('hold', on || dragging.value);
  }

  function startSlide(row: (typeof rows.value)[number], event: PointerEvent) {
    dragging.value = true;
    emit('hold', true);
    slide(row, event, () => {
      dragging.value = false;
      emit('hold', hovered.value);
    });
  }

  function pull(event: PointerEvent) {
    const handle = event.currentTarget as HTMLElement;
    const from = event.clientY;
    handle.setPointerCapture(event.pointerId);
    const onMove = (move: PointerEvent) => {
      if (from - move.clientY < PULL_PX) return;
      stop();
      emit('open', props.shown.key);
    };
    const stop = () => {
      handle.removeEventListener('pointermove', onMove);
      handle.removeEventListener('pointerup', stop);
      handle.removeEventListener('pointercancel', stop);
    };
    handle.addEventListener('pointermove', onMove);
    handle.addEventListener('pointerup', stop);
    handle.addEventListener('pointercancel', stop);
  }
</script>

<template>
  <section
    class="sound-drawer"
    aria-label="Volume"
    @mousedown.prevent
    @pointerenter="setHovered(true)"
    @pointerleave="setHovered(false)"
  >
    <button
      type="button"
      class="sound-drawer-handle"
      aria-label="Open the Sound view"
      @pointerdown.prevent="pull"
      @click="emit('open', shown.key)"
    >
      <span />
    </button>
    <div role="listbox" aria-label="Levels">
      <MixerRow
        v-for="row in rows"
        :key="row.key"
        :row="row"
        :selected="row.key === shown.key"
        :master="false"
        @wheel="wheel(row, $event)"
        @slide="startSlide(row, $event)"
        @mute="toggleMute(row)"
      />
    </div>
    <div class="sound-drawer-foot">
      <span>Drag, scroll or mute here</span>
      <button type="button" class="sound-drawer-more" @click="emit('open', shown.key)">
        {{ appCount === 1 ? '1 app' : `All ${appCount} apps` }}
        <kbd>Tab</kbd>
      </button>
    </div>
  </section>
</template>
