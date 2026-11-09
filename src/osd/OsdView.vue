<script setup lang="ts">
  import './osd.css';
  import { computed, onMounted, onUnmounted, ref } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import RowIcon from '../bar/RowIcon.vue';
  import SpeakerIcon from '../bar/SpeakerIcon.vue';
  import masterSpeaker from './master-speaker.png';
  import { osdHold, osdOpenSound, osdWheel, soundSetMuted, soundSetVolume } from '../api';
  import type { VolumeShown } from '../types';

  // The volume overlay, at the screen's corner while the bar is closed: the keys' target as
  // its icon, a slider and its level, as Maestro drew it. Tab (Shift+Tab back) or Alt+volume
  // steps through the sources. Rust shows and hides the window, and keeps it up while the
  // pointer is on it; the wheel over it or a drag along its slider moves the level, a click
  // on the icon mutes, and a click anywhere else opens the Sound view on the target.

  const shown = ref<VolumeShown | null>(null);
  /** Bumped when the overlay comes back after hiding, so the fade-in plays then and not on
   *  every press while it's up. */
  const pulse = ref(0);
  /** Rust hides the window this long after the last press (volume_keys.rs SHOWN_FOR). */
  const SHOWN_FOR_MS = 1600;
  let lastShown = 0;
  let unlisten: UnlistenFn | null = null;

  onMounted(async () => {
    document.documentElement.classList.add('osd-page');
    unlisten = await listen<VolumeShown>('osd-show', (event) => {
      const now = performance.now();
      if (now - lastShown > SHOWN_FOR_MS) pulse.value += 1;
      lastShown = now;
      shown.value = event.payload;
    });
  });

  onUnmounted(() => unlisten?.());

  const target = computed(
    () => shown.value?.levels.find((level) => level.key === shown.value?.key) ?? null
  );
  const percent = computed(() => Math.round((target.value?.volume ?? 0) * 100));

  function wheel(event: WheelEvent) {
    event.preventDefault();
    void osdWheel(event.deltaY < 0 ? 0.02 : -0.02);
  }

  /** Changes the target here at once; the caller tells the system. */
  function update(change: { volume?: number; muted?: boolean }) {
    const now = shown.value;
    const level = target.value;
    if (!now || !level) return;
    shown.value = {
      ...now,
      levels: now.levels.map((item) => (item.key === level.key ? { ...item, ...change } : item)),
    };
  }

  function setLevel(volume: number) {
    const level = target.value;
    if (!level) return;
    volume = Math.min(1, Math.max(0, Math.round(volume * 100) / 100));
    const wasMuted = level.muted;
    update({ volume, muted: false });
    void soundSetVolume(level.key, volume).catch(() => {});
    if (wasMuted) void soundSetMuted(level.key, false).catch(() => {});
  }

  function toggleMute() {
    const level = target.value;
    if (!level) return;
    update({ muted: !level.muted });
    void soundSetMuted(level.key, !level.muted).catch(() => {});
  }

  function slide(event: PointerEvent) {
    const track = event.currentTarget as HTMLElement;
    const place = (move: PointerEvent) => {
      const box = track.getBoundingClientRect();
      setLevel((move.clientX - box.left) / Math.max(1, box.width));
    };
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
</script>

<template>
  <div
    v-if="target"
    :key="pulse"
    class="osd"
    :class="{ muted: target.muted }"
    role="status"
    :aria-label="`${target.name} ${target.muted ? 'muted' : `${percent}%`}`"
    @wheel="wheel"
    @pointerenter="osdHold(true)"
    @pointerleave="osdHold(false)"
    @mousedown.prevent
    @click="osdOpenSound(target.key)"
  >
    <button
      type="button"
      class="osd-source"
      :aria-label="target.muted ? `Unmute ${target.name}` : `Mute ${target.name}`"
      @click.stop="toggleMute"
    >
      <img
        v-if="target.key === 'master'"
        class="osd-master"
        :src="masterSpeaker"
        alt=""
        draggable="false"
      />
      <RowIcon v-else :key="target.key" :icon="target.icon" :label="target.name" :size="24" />
      <SpeakerIcon :volume="target.volume" :muted="target.muted" :size="18" />
    </button>
    <div
      class="osd-slider"
      role="slider"
      :aria-valuenow="percent"
      aria-valuemin="0"
      aria-valuemax="100"
      @pointerdown.prevent="slide"
      @click.stop
    >
      <span class="osd-track" />
      <span class="osd-fill" :style="{ width: `${percent}%` }" />
      <span class="osd-knob" :style="{ left: `${percent}%` }" />
    </div>
    <span class="osd-value">{{ percent }}</span>
  </div>
</template>
