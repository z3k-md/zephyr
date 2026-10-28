<script setup lang="ts">
  import { computed } from 'vue';
  import type { NetworkSummary, SpeedResult, Suggestion } from '../types';
  import KeyHints from '../components/KeyHints.vue';
  import { GRADE_NAMES, headline, lastTestLine, ms, note, sparkPath } from './network';

  // The network card, the first row for "ping", "network" and the like (Enter opens the
  // Network view), and for "ip", where the local address leads (Enter copies it). Both draw
  // the last 15 minutes of latency at the right.
  const props = defineProps<{
    row: Suggestion;
    selected: boolean;
    summary: NetworkSummary | null;
    lastSpeed: SpeedResult | null;
  }>();

  const ip = computed(() => props.row.target === 'ip');
  const spark = computed(() => sparkPath(props.summary?.spark ?? [], 220, 56));
  /** Stretches with probes lost, as faint columns behind the line. */
  const dark = computed(() => {
    const lost = props.summary?.sparkLost ?? [];
    const step = 220 / Math.max(1, lost.length - 1);
    return lost.flatMap((any, index) =>
      any ? [{ x: index * step - step / 2, width: Math.max(2, step) }] : []
    );
  });

  const pill = computed(() => {
    if (ip.value) return (props.row.subtitle ?? '').split(' · ')[0] || 'Local';
    const summary = props.summary;
    if (!summary || !summary.watching) return 'Off';
    if (summary.offline) return 'Offline';
    return summary.median === null ? '…' : GRADE_NAMES[summary.grade];
  });
  const tone = computed(() => {
    const summary = props.summary;
    if (ip.value || !summary || !summary.watching || summary.median === null) return '';
    return summary.offline ? 'poor' : summary.grade;
  });

  const big = computed(() => {
    if (ip.value) return props.row.label;
    return props.summary ? headline(props.summary) : 'Measuring…';
  });

  const caption = computed(() => (ip.value ? 'Local IP' : 'Last 15 min · 1.1.1.1'));

  const sub = computed(() => {
    const summary = props.summary;
    if (ip.value) {
      const parts = [
        summary?.gateway ? `Router ${summary.gateway}` : '',
        summary?.dns ? `DNS ${summary.dns}` : '',
        summary?.watching && summary.median !== null
          ? `${GRADE_NAMES[summary.grade]} · ${ms(summary.median)} ms`
          : '',
      ];
      return parts.filter(Boolean).join(' · ');
    }
    if (!summary) return '';
    return [note(summary), lastTestLine(props.lastSpeed)].filter(Boolean).join(' · ');
  });

  const hints = computed(() =>
    ip.value
      ? [
          { keys: 'enter', label: 'Copy' },
          { keys: 'mod+enter', label: 'Network' },
        ]
      : [{ keys: 'enter', label: 'Open' }]
  );
</script>

<template>
  <section class="net-card" :class="{ selected }" aria-label="Network">
    <div class="net-card-text">
      <div class="net-card-head">
        <span class="net-pill" :class="tone">{{ pill }}</span>
        <span class="net-card-caption">{{ caption }}</span>
        <KeyHints v-if="selected" :hints="hints" />
      </div>
      <div class="net-card-big">{{ big }}</div>
      <div v-if="sub" class="net-card-sub">{{ sub }}</div>
    </div>
    <svg
      class="net-card-spark"
      viewBox="0 0 220 56"
      width="220"
      height="56"
      aria-label="Latency, last 15 minutes"
    >
      <rect
        v-for="mark in dark"
        :key="mark.x"
        class="lost"
        :x="mark.x"
        y="0"
        :width="mark.width"
        height="56"
      />
      <path :d="spark" />
    </svg>
  </section>
</template>
