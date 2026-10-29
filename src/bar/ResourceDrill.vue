<script setup lang="ts">
  import { computed } from 'vue';
  import { bytes } from '../format';
  import type { CoreHeat, MemoryBreakdown } from '../types';

  // The CPU and Memory tiles' drill-ins, in place of the list: each logical processor over
  // the last 15 minutes as a heatmap, or where the memory is. Esc goes back to the list.
  const props = defineProps<{
    kind: 'cpu' | 'memory';
    heat: CoreHeat | null;
    memory: MemoryBreakdown | null;
  }>();

  const W = 600;

  const grid = computed(() => {
    const heat = props.heat;
    if (!heat || !heat.cores) return null;
    // Rows shrink to fit every processor in the space the list had.
    const rowHeight = Math.max(6, Math.min(16, Math.floor(232 / heat.cores)));
    const columnWidth = W / Math.max(1, heat.columns.length);
    const cells: { x: number; y: number; load: number }[] = [];
    heat.columns.forEach((column, index) => {
      column?.forEach((load, core) => {
        cells.push({ x: index * columnWidth, y: core * rowHeight, load });
      });
    });
    return {
      cells,
      rowHeight,
      columnWidth,
      height: heat.cores * rowHeight,
      labels: Array.from({ length: heat.cores }, (_, core) => core).filter(
        (core) => heat.cores <= 16 || core % 2 === 0
      ),
    };
  });

  /** Idle reads as the panel, busy as the accent, pegged as hot. */
  function fill(load: number): string {
    if (load >= 90) return `rgba(255, 120, 90, ${0.55 + (load - 90) / 40})`;
    return `rgba(91, 156, 255, ${0.06 + (load / 100) * 0.8})`;
  }

  const parts = computed(() => {
    const memory = props.memory;
    if (!memory || !memory.total) return [];
    // macOS splits it as Activity Monitor does.
    const list =
      memory.wired !== undefined
        ? [
            { label: 'App memory', value: memory.inUse, color: 'rgba(255, 255, 255, 0.78)' },
            { label: 'Wired', value: memory.wired, color: 'var(--warn)' },
            { label: 'Compressed', value: memory.compressed ?? 0, color: '#d98a5f' },
            { label: 'Cached files', value: memory.standby, color: 'rgba(255, 255, 255, 0.32)' },
            { label: 'Free', value: memory.free, color: 'rgba(255, 255, 255, 0.1)' },
          ]
        : [
            { label: 'In use', value: memory.inUse, color: 'rgba(255, 255, 255, 0.78)' },
            { label: 'Modified', value: memory.modified, color: 'var(--warn)' },
            { label: 'Standby', value: memory.standby, color: 'rgba(255, 255, 255, 0.32)' },
            { label: 'Free', value: memory.free, color: 'rgba(255, 255, 255, 0.1)' },
          ];
    return list.map((part) => ({ ...part, share: (part.value / memory.total) * 100 }));
  });
</script>

<template>
  <section class="res-drill" :aria-label="kind === 'cpu' ? 'Processors' : 'Memory'">
    <template v-if="kind === 'cpu'">
      <div class="res-drill-head">
        <b>Processors</b>
        <span v-if="heat">{{ heat.cores }} logical · last 15 minutes</span>
      </div>
      <div v-if="grid" class="res-heat-wrap" :style="{ height: `${grid.height}px` }">
        <div class="res-heat-labels" aria-hidden="true">
          <span
            v-for="core in grid.labels"
            :key="core"
            :style="{ top: `${core * grid.rowHeight}px`, lineHeight: `${grid.rowHeight}px` }"
            >{{ core }}</span
          >
        </div>
        <svg
          class="res-heat"
          :viewBox="`0 0 ${W} ${grid.height}`"
          preserveAspectRatio="none"
          role="img"
          aria-label="Load on each processor over time"
        >
          <rect
            v-for="cell in grid.cells"
            :key="`${cell.x}-${cell.y}`"
            :x="cell.x"
            :y="cell.y + 0.5"
            :width="grid.columnWidth + 0.3"
            :height="grid.rowHeight - 1"
            :fill="fill(cell.load)"
          />
        </svg>
      </div>
      <div v-if="heat?.now.length" class="res-drill-now">
        Now:
        <span v-for="(load, core) in heat.now" :key="core" :class="{ hot: load >= 90 }">{{
          load
        }}</span>
      </div>
    </template>
    <template v-else>
      <div class="res-drill-head">
        <b>Memory</b>
        <span v-if="memory">{{ bytes(memory.total, 0) }} installed</span>
      </div>
      <template v-if="memory && parts.length">
        <div class="res-stack">
          <span
            v-for="part in parts"
            :key="part.label"
            :style="{ width: `${part.share}%`, background: part.color }"
          />
        </div>
        <ul class="res-legend">
          <li v-for="part in parts" :key="part.label">
            <i :style="{ background: part.color }" />{{ part.label }}
            <b>{{ bytes(part.value) }}</b>
          </li>
        </ul>
        <ul v-if="memory.wired === undefined" class="res-legend res-legend-more">
          <li>
            Committed <b>{{ bytes(memory.committed) }} of {{ bytes(memory.commitLimit) }}</b>
          </li>
          <li>
            Paged pool <b>{{ bytes(memory.pagedPool) }}</b>
          </li>
          <li>
            Non-paged pool <b>{{ bytes(memory.nonpagedPool) }}</b>
          </li>
        </ul>
        <p v-if="memory.wired !== undefined" class="res-drill-note">
          Cached files are kept from disk, and macOS hands them back the moment an app needs the
          memory.
        </p>
        <p v-else class="res-drill-note">
          Standby is cache Windows hands back the moment an app needs it; Modified is waiting to be
          written to disk first.
        </p>
      </template>
      <p v-else class="res-drill-note">The breakdown isn't available on this computer yet.</p>
    </template>
  </section>
</template>
