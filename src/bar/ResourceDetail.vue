<script setup lang="ts">
  import { computed } from 'vue';
  import RowIcon from './RowIcon.vue';
  import Keys from '../components/Keys.vue';
  import { ago, bytes, elapsed } from '../format';
  import { isMac } from '../keys';
  import type { ResourceDetail, ResourceRow } from '../types';

  // Show info (Shift+Space, or Enter on a process with no window): one row's CPU and memory
  // over the time Resources has been open, its facts, and what can be done to it.
  const props = defineProps<{ row: ResourceRow; detail: ResourceDetail | null }>();
  const emit = defineEmits<{
    switch: [];
    quit: [force: boolean];
    reveal: [];
    close: [];
  }>();

  const W = 620;
  const H = 96;
  /** As far back as the tiles go. */
  const WINDOW = 900;

  const chart = computed(() => {
    const points = props.detail?.history ?? [];
    if (points.length < 2) return null;
    const x = (age: number) => W - (Math.min(age, WINDOW) / WINDOW) * W;
    // CPU on a fixed scale (a full core, or the whole machine on Windows); memory on its own.
    const cpuTop = Math.max(100, ...points.map((point) => point[1]));
    const memoryTop = Math.max(1, ...points.map((point) => point[2])) * 1.1;
    const line = (value: (point: [number, number, number]) => number, top: number) =>
      points
        .map(
          (point, index) =>
            `${index ? 'L' : 'M'}${x(point[0]).toFixed(1)} ${(H - 2 - (value(point) / top) * (H - 6)).toFixed(1)}`
        )
        .join('');
    const cpu = line((point) => point[1], cpuTop);
    const first = x(points[0][0]).toFixed(1);
    return {
      cpu,
      cpuArea: `${cpu}L${W} ${H}L${first} ${H}Z`,
      memory: line((point) => point[2], memoryTop),
      since: points[0][0],
    };
  });

  const started = computed(() => {
    const at = props.detail?.started;
    if (!at) return '—';
    return ago(at);
  });

  const facts = computed(() => {
    const row = props.row;
    const detail = props.detail;
    return [
      ['Process ID', detail?.pid ? String(detail.pid) : row.pids.join(' ')],
      ['Parent', detail?.parent || '—'],
      ['User', row.user || '—'],
      ['Started', started.value],
      ['Read', detail ? bytes(detail.read) : '—'],
      ['Written', detail ? bytes(detail.written) : '—'],
    ];
  });

  const quitLabel = computed(() =>
    props.row.relaunch ? (isMac ? 'Relaunch' : 'Restart') : 'Quit'
  );
</script>

<template>
  <section class="res-detail" aria-label="Process details">
    <div class="res-detail-head">
      <RowIcon :icon="row.icon" :label="row.name" :size="32" />
      <div class="res-detail-title">
        <b>{{ row.name }}</b>
        <span>{{ row.path || detail?.command || 'Path needs administrator rights' }}</span>
      </div>
      <button type="button" class="chip" @mousedown.prevent @click="emit('close')">
        <Keys keys="shift+space" small /> Close
      </button>
    </div>
    <svg
      v-if="chart"
      class="res-detail-chart"
      :viewBox="`0 0 ${W} ${H}`"
      preserveAspectRatio="none"
      aria-label="CPU and memory while Resources has been open"
    >
      <line class="grid" x1="0" :x2="W" :y1="H / 2" :y2="H / 2" />
      <path class="cpu-area" :d="chart.cpuArea" />
      <path class="cpu" :d="chart.cpu" />
      <path class="memory" :d="chart.memory" />
    </svg>
    <p v-else class="res-detail-empty">The graph fills in as Resources stays open.</p>
    <div class="res-detail-legend">
      <span><i class="cpu" />CPU {{ row.cpu === null ? '—' : `${row.cpu.toFixed(1)}%` }}</span>
      <span><i class="memory" />Memory {{ row.memory === null ? '—' : bytes(row.memory) }}</span>
      <span v-if="chart" class="since">last {{ elapsed(chart.since) }}</span>
    </div>
    <dl class="res-detail-facts">
      <div v-for="[label, value] in facts" :key="label">
        <dt>{{ label }}</dt>
        <dd>{{ value }}</dd>
      </div>
    </dl>
    <div class="res-detail-actions">
      <button type="button" class="primary" @mousedown.prevent @click="emit('switch')">
        Switch to
      </button>
      <button type="button" @mousedown.prevent @click="emit('quit', false)">{{ quitLabel }}</button>
      <button
        type="button"
        :disabled="row.protected"
        :title="row.protected ? 'Keeps the system running' : undefined"
        @mousedown.prevent
        @click="emit('quit', true)"
      >
        Force quit
      </button>
      <button v-if="row.path" type="button" @mousedown.prevent @click="emit('reveal')">
        Show in {{ isMac ? 'Finder' : 'Explorer' }}
      </button>
    </div>
  </section>
</template>
