<script setup lang="ts">
  import { computed } from 'vue';
  import './speedPanel.css';
  import { vTip } from '../tip';
  import { BLOAT_TIP, bloatWord, mbps, ms } from '../bar/network';
  import type { SpeedProgress, SpeedResult } from '../types';

  // A speed test as it runs (phase, a progress bar, the live throughput and ping) and its
  // result once done. The Network view and Settings both show it.
  const props = defineProps<{ testing: SpeedProgress | null; result: SpeedResult | null }>();

  const PHASES: Record<SpeedProgress['phase'], string> = {
    ping: 'Measuring idle ping',
    download: 'Download',
    upload: 'Upload',
  };

  /** "Ping 12 ms idle, +45 ms while busy". */
  const pingLine = computed(() => {
    const done = props.result;
    if (!done || done.idleMs === null) return '';
    const loaded = Math.max(done.downMs ?? 0, done.upMs ?? 0);
    const added = loaded ? Math.max(0, loaded - done.idleMs) : null;
    return added === null
      ? `Ping ${ms(done.idleMs)} ms idle`
      : `Ping ${ms(done.idleMs)} ms idle, +${Math.round(added)} ms while busy`;
  });
</script>

<template>
  <div v-if="testing || result" class="speed-panel" aria-live="polite">
    <template v-if="testing">
      <div class="speed-panel-head">
        <span>{{ PHASES[testing.phase] }}</span>
        <span class="speed-panel-bar"><span :style="{ width: `${testing.done * 100}%` }" /></span>
      </div>
      <div class="speed-panel-main">
        <b>{{ testing.mbps !== null ? mbps(testing.mbps) : '—' }}</b> Mb/s
        <span>ping {{ ms(testing.ping) }} ms</span>
      </div>
    </template>
    <template v-else-if="result">
      <div class="speed-panel-head"><span>Speed test</span></div>
      <div class="speed-panel-main">
        <b>{{ mbps(result.downMbps) }}</b> ↓ <b>{{ mbps(result.upMbps) }}</b> ↑ Mb/s
        <span
          v-tip="BLOAT_TIP"
          class="speed-panel-grade"
          :class="`grade-${result.bloat}`"
          tabindex="0"
        >
          Lag under load <b>{{ result.bloat }}</b> {{ bloatWord(result.bloat) }}
        </span>
      </div>
      <div v-if="pingLine" class="speed-panel-sub">{{ pingLine }}</div>
    </template>
  </div>
</template>
