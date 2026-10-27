<script setup lang="ts">
  import { computed, ref } from 'vue';
  import { errorMessage, saveInstantStartup, saveSettings } from '../api';
  import type { Snapshot } from '../types';
  import InfoTip from '../components/InfoTip.vue';

  // Run at boot, as one choice: off, on, or (Windows) high-priority, which starts from a
  // Task Scheduler task instead of the startup list Windows holds back for a few seconds
  // after sign-in. Defender quarantined Zephyr for that task once, so it says so.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  type Mode = 'off' | 'on' | 'fast';
  const isWindows = navigator.userAgent.includes('Windows');
  const OPTIONS: { id: Mode; label: string }[] = [
    { id: 'off', label: 'Off' },
    { id: 'on', label: 'On' },
    ...(isWindows ? [{ id: 'fast' as const, label: 'High-priority' }] : []),
  ];
  const CAVEAT =
    'High-priority starts Zephyr from a scheduled task so it’s ready sooner. Windows ' +
    'Defender or other antivirus may flag it; if it does, add an exception for Zephyr.';

  const busy = ref(false);
  const error = ref<string | null>(null);
  const mode = computed<Mode>(() => {
    if (!props.snapshot.launchAtStartup) return 'off';
    return isWindows && props.snapshot.instantStartup ? 'fast' : 'on';
  });

  async function choose(next: Mode) {
    if (next === mode.value || busy.value) return;
    busy.value = true;
    try {
      const current = props.snapshot;
      let snapshot = current;
      const boot = next !== 'off';
      if (boot !== current.launchAtStartup) {
        snapshot = await saveSettings(current.summonShortcut, boot);
      }
      const fast = next === 'fast';
      if (isWindows && fast !== snapshot.instantStartup) {
        snapshot = await saveInstantStartup(fast);
      }
      emit('apply', snapshot);
      error.value = null;
    } catch (err) {
      error.value = errorMessage(err);
    } finally {
      busy.value = false;
    }
  }
</script>

<template>
  <div class="boot-choice">
    <div class="segments" role="radiogroup" aria-label="Run at boot">
      <button
        v-for="option in OPTIONS"
        :key="option.id"
        type="button"
        role="radio"
        class="segment"
        :class="{ chosen: mode === option.id }"
        :aria-checked="mode === option.id"
        :disabled="busy"
        @click="choose(option.id)"
      >
        {{ option.label }}
      </button>
    </div>
    <InfoTip v-if="isWindows" :text="CAVEAT" />
    <p v-if="error" class="notice">{{ error }}</p>
  </div>
</template>
