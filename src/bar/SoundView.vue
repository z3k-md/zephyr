<script setup lang="ts">
  import { computed, onMounted, onUnmounted, ref } from 'vue';
  import { vTip } from '../tip';
  import { usePoll } from '../poll';
  import KeyHints from '../components/KeyHints.vue';
  import BackButton from './BackButton.vue';
  import MixerRow from './MixerRow.vue';
  import NowPlayingStrip from './NowPlayingStrip.vue';
  import ActionMenu from '../components/ActionMenu.vue';
  import { MENU_HINTS, useActionMenu, type MenuAction } from '../components/actionMenu';
  import { errorMessage, soundSetDevice, soundSnapshot } from '../api';
  import { STEP, useMixer, type MixerLevel } from './mixer';
  import type { MediaControl, NowPlaying, Sound } from '../types';

  // The Sound view: what's playing, the output device and its level, and every app's level in
  // the volume mixer, with how loud each is right now. The first row is the device's own
  // level; the arrows pick a row and move it, M mutes it, 1–9 and 0 jump to 10–100%, and Tab
  // moves to the next output device.
  defineProps<{ track: NowPlaying | null; positionMs: number | null }>();
  const emit = defineEmits<{ exit: []; media: [MediaControl]; focusMedia: [] }>();

  const LIST_HINTS = [
    { keys: 'left', label: '' },
    { keys: 'right', label: 'level' },
    { keys: 'm', label: 'mute' },
    { keys: 'tab', label: 'device' },
    { keys: 'mod+k', label: 'actions' },
  ];
  /** Fast enough for the meters to move with the sound. */
  const POLL_MS = 120;
  const BIG_STEP = 0.1;

  /** A row: the device's level (`master`) or an app's. */
  interface Row extends MixerLevel {
    detail: string;
    peak: number;
    active: boolean;
  }

  const sound = ref<Sound | null>(null);
  const loaded = ref(false);
  const notice = ref<string | null>(null);
  const selected = ref(0);
  /** A row asked for by key (from the drawer or Alt+Mute), picked once the rows load. */
  const wanted = ref<string | null>(null);
  const live = ref(false);
  const {
    withHeld,
    apply,
    nudge,
    toggleMute,
    slide: slideRow,
    wheel,
  } = useMixer((message) => (notice.value = message));
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
    if (wanted.value) select(wanted.value);
  }

  /** Picks the row for `key` (master or an app), now or once the rows load. */
  function select(key: string) {
    const at = rows.value.findIndex((row) => row.key === key);
    if (at >= 0) {
      selected.value = at;
      wanted.value = null;
    } else {
      wanted.value = key;
    }
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
    return withHeld(list);
  });

  /** Where "Quiet" starts: the first app that isn't playing. */
  const quietFrom = computed(() => {
    const index = rows.value.findIndex((row, at) => at > 0 && !row.active);
    return index;
  });

  const current = computed(() => rows.value[Math.min(selected.value, rows.value.length - 1)]);

  function slide(row: Row, event: PointerEvent) {
    selected.value = rows.value.findIndex((item) => item.key === row.key);
    slideRow(row, event);
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

  defineExpose({ onKey, start, stop, select });
</script>

<template>
  <div class="sound">
    <div class="input-row">
      <BackButton @back="emit('exit')" />
      <span class="chip scope-label">Sound</span>
      <span class="net-title">{{ sound?.device?.detail ?? '' }}</span>
      <span class="res-live" :class="{ on: live && !!sound }" aria-hidden="true" />
    </div>

    <div v-if="track" class="sound-now" @mousedown.prevent>
      <NowPlayingStrip
        :track="track"
        :position-ms="positionMs"
        @toggle="emit('media', 'toggle')"
        @next="emit('media', 'next')"
        @previous="emit('media', 'previous')"
        @focus="emit('focusMedia')"
      />
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
        <MixerRow
          :row="row"
          :selected="index === selected"
          :master="index === 0"
          @hover="selected = index"
          @wheel="wheel(row, $event)"
          @slide="slide(row, $event)"
          @mute="toggleMute(row)"
        >
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
        </MixerRow>
      </template>
    </div>

    <footer class="footer">
      <KeyHints :hints="actionsOpen ? MENU_HINTS : LIST_HINTS" />
    </footer>
  </div>
</template>
