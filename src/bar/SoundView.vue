<script setup lang="ts">
  import { computed, onMounted, onUnmounted, ref } from 'vue';
  import RowIcon from './RowIcon.vue';
  import { GLYPHS } from './glyphs';
  import { vTip } from '../tip';
  import { usePoll } from '../poll';
  import KeyHints from '../components/KeyHints.vue';
  import BackButton from './BackButton.vue';
  import ActionMenu from '../components/ActionMenu.vue';
  import { MENU_HINTS, useActionMenu, type MenuAction } from '../components/actionMenu';
  import {
    errorMessage,
    soundSetDevice,
    soundSetMuted,
    soundSetVolume,
    soundSnapshot,
  } from '../api';
  import type { Sound } from '../types';

  // The Sound view: the output device and its level, and every app's level in the volume
  // mixer, with how loud each is right now. The first row is the device's own level; the arrows
  // pick a row and move it, M mutes it, 1–9 and 0 jump to 10–100%, and Tab moves to the next
  // output device.
  const emit = defineEmits<{ exit: [] }>();

  const LIST_HINTS = [
    { keys: 'left', label: '' },
    { keys: 'right', label: 'level' },
    { keys: 'm', label: 'mute' },
    { keys: 'tab', label: 'device' },
    { keys: 'mod+k', label: 'actions' },
  ];
  /** Fast enough for the meters to move with the sound. */
  const POLL_MS = 120;
  /** A row just changed here keeps its new level this long, so a refresh already on its way
   *  doesn't pull the slider back. */
  const HOLD_MS = 800;
  const STEP = 0.02;
  const BIG_STEP = 0.1;

  /** A row: the device's level (`master`) or an app's. */
  interface Row {
    key: string;
    name: string;
    detail: string;
    icon: string;
    volume: number;
    muted: boolean;
    peak: number;
    active: boolean;
  }

  const sound = ref<Sound | null>(null);
  const loaded = ref(false);
  const notice = ref<string | null>(null);
  const selected = ref(0);
  const live = ref(false);
  /** Levels set here and not yet confirmed by a refresh, by row key. */
  const held = ref<Record<string, { volume: number; muted: boolean; until: number }>>({});
  const menu = useActionMenu(() => actions.value);
  const { open: actionsOpen, index: actionIndex } = menu;

  const poll = usePoll(tick, POLL_MS, live);
  onMounted(start);
  onUnmounted(stop);

  /** Refreshes while the bar shows; the bar stops it when it hides. */
  function start() {
    if (!live.value) live.value = true;
    else poll.restart();
  }

  function stop() {
    live.value = false;
  }

  async function tick() {
    try {
      sound.value = await soundSnapshot();
    } catch (error) {
      notice.value = errorMessage(error);
    }
    loaded.value = true;
  }

  const rows = computed<Row[]>(() => {
    const now = sound.value;
    if (!now) return [];
    const list: Row[] = [
      {
        key: 'master',
        name: now.device?.name ?? 'Output',
        detail: now.device?.detail ?? '',
        icon: 'glyph:speaker',
        volume: now.volume,
        muted: now.muted,
        peak: now.peak,
        active: true,
      },
      ...now.apps.map((app) => ({ ...app, detail: '' })),
    ];
    const time = performance.now();
    return list.map((row) => {
      const hold = held.value[row.key];
      return hold && hold.until > time ? { ...row, volume: hold.volume, muted: hold.muted } : row;
    });
  });

  /** Where "Quiet" starts: the first app that isn't playing. */
  const quietFrom = computed(() => {
    const index = rows.value.findIndex((row, at) => at > 0 && !row.active);
    return index;
  });

  const current = computed(() => rows.value[Math.min(selected.value, rows.value.length - 1)]);

  function percent(value: number): number {
    return Math.round(value * 100);
  }

  /** Sets a row's level and mute here at once, then on the system. */
  async function apply(row: Row, volume: number, muted: boolean) {
    volume = Math.min(1, Math.max(0, Math.round(volume * 100) / 100));
    held.value = {
      ...held.value,
      [row.key]: { volume, muted, until: performance.now() + HOLD_MS },
    };
    try {
      if (volume !== row.volume) await soundSetVolume(row.key, volume);
      if (muted !== row.muted || (volume !== row.volume && muted)) {
        await soundSetMuted(row.key, muted);
      }
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  function nudge(row: Row, delta: number) {
    // Turning a muted row up unmutes it, as Windows' own slider does.
    void apply(row, row.volume + delta, row.muted && delta <= 0);
  }

  function toggleMute(row: Row) {
    void apply(row, row.volume, !row.muted);
  }

  /** The level under the pointer, while it presses or drags along a slider. */
  function slide(row: Row, event: PointerEvent) {
    const track = event.currentTarget as HTMLElement;
    const place = (move: PointerEvent) => {
      const box = track.getBoundingClientRect();
      const level = (move.clientX - box.left) / Math.max(1, box.width);
      void apply(row, level, false);
    };
    selected.value = rows.value.findIndex((item) => item.key === row.key);
    track.setPointerCapture(event.pointerId);
    place(event);
    const onMove = (move: PointerEvent) => place(move);
    const onUp = () => {
      track.removeEventListener('pointermove', onMove);
      track.removeEventListener('pointerup', onUp);
      track.removeEventListener('pointercancel', onUp);
    };
    track.addEventListener('pointermove', onMove);
    track.addEventListener('pointerup', onUp);
    track.addEventListener('pointercancel', onUp);
  }

  /** The wheel over a row: a notch is one step. */
  function wheel(row: Row, event: WheelEvent) {
    event.preventDefault();
    nudge(row, event.deltaY < 0 ? STEP : -STEP);
  }

  async function useDevice(id: string) {
    if (id === sound.value?.device?.id) return;
    try {
      await soundSetDevice(id);
      selected.value = 0;
      poll.restart();
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  /** Tab: the next output device (Shift+Tab, the one before). */
  function cycleDevice(back: boolean) {
    const now = sound.value;
    if (!now || now.devices.length < 2) return;
    const at = now.devices.findIndex((device) => device.id === now.device?.id);
    const next = (at + (back ? -1 : 1) + now.devices.length) % now.devices.length;
    void useDevice(now.devices[next].id);
  }

  const actions = computed<MenuAction[]>(() => {
    const row = current.value;
    const list: MenuAction[] = [];
    if (row) {
      list.push({
        label: row.muted ? `Unmute ${row.name}` : `Mute ${row.name}`,
        keys: 'm',
        run: () => toggleMute(row),
      });
      if (row.volume < 1)
        list.push({ label: `Set ${row.name} to 100%`, keys: '0', run: () => apply(row, 1, false) });
    }
    for (const device of sound.value?.devices ?? []) {
      if (device.id === sound.value?.device?.id) continue;
      list.push({ label: `Play through ${device.name}`, run: () => useDevice(device.id) });
    }
    return list;
  });

  /** Called by the bar for every key while the Sound view shows. */
  function onKey(event: KeyboardEvent) {
    if (menu.onKey(event)) return;
    const modifier = event.metaKey || event.ctrlKey || event.altKey;
    if (event.key === 'Escape' || (event.key === 'Backspace' && !event.repeat)) {
      event.preventDefault();
      if (notice.value) notice.value = null;
      else emit('exit');
      return;
    }
    if (event.key === 'Tab') {
      event.preventDefault();
      if (!event.repeat) cycleDevice(event.shiftKey);
      return;
    }
    const count = rows.value.length;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (count)
        selected.value = (selected.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
      return;
    }
    if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      selected.value = event.key === 'Home' ? 0 : Math.max(0, count - 1);
      return;
    }
    const row = current.value;
    if (!row) return;
    if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
      event.preventDefault();
      const size = event.shiftKey ? BIG_STEP : STEP;
      nudge(row, event.key === 'ArrowRight' ? size : -size);
      return;
    }
    if (modifier) return;
    if (event.key === 'm' || event.key === 'M' || event.key === ' ' || event.key === 'Enter') {
      event.preventDefault();
      if (!event.repeat) toggleMute(row);
      return;
    }
    if (/^[0-9]$/.test(event.key)) {
      event.preventDefault();
      const level = event.key === '0' ? 1 : Number(event.key) / 10;
      void apply(row, level, false);
    }
  }

  defineExpose({ onKey, start, stop });
</script>

<template>
  <div class="sound">
    <div class="input-row">
      <BackButton @back="emit('exit')" />
      <span class="chip scope-label">Sound</span>
      <span class="net-title">{{ sound?.device?.detail ?? '' }}</span>
      <span class="res-live" :class="{ on: live && !!sound }" aria-hidden="true" />
    </div>

    <p v-if="notice" class="notice">{{ notice }}</p>

    <ActionMenu
      v-if="actionsOpen"
      id="sound-actions"
      class="res-actions"
      :actions="actions"
      :index="actionIndex"
      @run="menu.run"
      @hover="menu.hover"
      @fit="menu.shown.value = $event"
    />

    <p v-else-if="loaded && !sound" class="sound-empty">
      Per-app volume isn't available on this computer yet.
    </p>

    <div v-else class="sound-list" role="listbox" aria-label="Volume">
      <template v-for="(row, index) in rows" :key="row.key">
        <div v-if="index === 1 && row.active" class="sound-section">Playing</div>
        <div v-if="index === quietFrom" class="sound-section">Quiet</div>
        <div
          class="sound-row"
          :class="{ selected: index === selected, muted: row.muted, master: index === 0 }"
          role="option"
          :aria-selected="index === selected"
          :aria-label="`${row.name}, ${row.muted ? 'muted' : `${percent(row.volume)}%`}`"
          @mousemove="selected = index"
          @wheel="wheel(row, $event)"
        >
          <RowIcon :icon="row.icon" :label="row.name" :size="20" />
          <span class="sound-name">
            {{ row.name }}
            <small v-if="index === 0 && (sound?.devices.length ?? 0) > 1">
              <button
                v-for="device in sound?.devices ?? []"
                :key="device.id"
                type="button"
                class="sound-device"
                :class="{ on: device.id === sound?.device?.id }"
                v-tip="device.detail"
                @mousedown.prevent
                @click="useDevice(device.id)"
              >
                {{ device.name }}
              </button>
            </small>
          </span>
          <span
            class="sound-slider"
            role="slider"
            :aria-valuenow="percent(row.volume)"
            aria-valuemin="0"
            aria-valuemax="100"
            @pointerdown.prevent="slide(row, $event)"
          >
            <span class="sound-track">
              <span
                class="sound-peak"
                :style="{ width: `${percent(row.muted ? 0 : row.peak)}%` }"
              />
            </span>
            <span class="sound-fill" :style="{ width: `${percent(row.volume)}%` }" />
            <span class="sound-knob" :style="{ left: `${percent(row.volume)}%` }" />
          </span>
          <span class="sound-pct">{{ percent(row.volume) }}%</span>
          <button
            type="button"
            class="sound-mute"
            :class="{ on: row.muted }"
            :aria-label="row.muted ? `Unmute ${row.name}` : `Mute ${row.name}`"
            v-tip="row.muted ? 'Unmute (M)' : 'Mute (M)'"
            @mousedown.prevent
            @click="toggleMute(row)"
          >
            <svg
              viewBox="0 0 24 24"
              aria-hidden="true"
              v-html="row.muted ? GLYPHS.speakerMuted : GLYPHS.speaker"
            />
          </button>
        </div>
      </template>
    </div>

    <footer class="footer">
      <KeyHints :hints="actionsOpen ? MENU_HINTS : LIST_HINTS" />
    </footer>
  </div>
</template>
