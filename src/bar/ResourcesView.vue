<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
  import RowIcon from './RowIcon.vue';
  import { GLYPHS } from './glyphs';
  import { vFit } from '../fit';
  import { bytes } from '../format';
  import {
    errorMessage,
    resourcesClosed,
    resourcesQuit,
    resourcesRelaunch,
    resourcesReveal,
    resourcesSnapshot,
    resourcesSwitch,
  } from '../api';
  import type { ResourcePoint, ResourceRow, ResourceSnapshot, ResourceSort } from '../types';

  // System Resources: the machine's CPU, memory and network over the last minute, and the
  // apps using them, live. Rows are apps (helpers counted in) or command-line programs; Enter
  // switches to one, ⌘⌫ quits it, ⌥⌘⌫ force quits it, each after an inline confirm.
  const emit = defineEmits<{ exit: []; ask: [question: string]; search: [text: string] }>();

  interface Action {
    label: string;
    keys?: string;
    /** The menu stays open after it runs. */
    keepOpen?: boolean;
    run: () => unknown;
  }

  const SORTS: { id: ResourceSort; label: string }[] = [
    { id: 'cpu', label: 'CPU' },
    { id: 'memory', label: 'Memory' },
    { id: 'disk', label: 'Disk' },
    { id: 'name', label: 'Name' },
  ];
  const COLUMNS = [SORTS[3], SORTS[0], SORTS[1], SORTS[2]];
  const RATES = [1000, 2000, 5000];
  /** After a key or the pointer moves the selection, the order holds this long. */
  const HOLD_MS = 2000;

  const isMac = navigator.userAgent.includes('Mac');
  const mod = isMac ? '⌘' : 'Ctrl+';
  const alt = isMac ? '⌥' : 'Alt+';
  const finder = isMac ? 'Finder' : 'Explorer';

  const inputEl = ref<HTMLInputElement | null>(null);
  const filter = ref('');
  const sort = ref<ResourceSort>('cpu');
  const data = ref<ResourceSnapshot | null>(null);
  const rows = ref<ResourceRow[]>([]);
  const selectedKey = ref('');
  const shown = ref(Infinity);
  const notice = ref<string | null>(null);
  /** The quit waiting for a second press, on the selected row. */
  const confirming = ref<{ key: string; force: boolean } | null>(null);
  const actionsOpen = ref(false);
  const actionIndex = ref(0);
  const actionsShown = ref(Infinity);
  const rate = ref(1000);
  const live = ref(false);

  let timer = 0;
  let generation = 0;
  let heldUntil = 0;

  const selectedIndex = computed(() => {
    const index = rows.value.findIndex((row) => row.key === selectedKey.value);
    return index < 0 ? 0 : index;
  });
  const current = computed<ResourceRow | undefined>(() => rows.value[selectedIndex.value]);

  onMounted(async () => {
    start();
    await nextTick();
    inputEl.value?.focus();
  });

  onUnmounted(stop);

  watch([filter, sort], () => {
    heldUntil = 0;
    confirming.value = null;
    void tick(false);
  });

  /** Ticks while the bar shows; the bar stops it when it hides. */
  function start() {
    live.value = true;
    void tick(true);
  }

  /** The backend lets the process table go once nothing has asked for it in a while. */
  function stop() {
    live.value = false;
    window.clearTimeout(timer);
    timer = 0;
    void resourcesClosed().catch(() => undefined);
  }

  // A row that sorts below what fits is out of sight, so the selection moves to the last row
  // shown rather than letting Enter or a quit act on something the user can't see.
  watch([selectedIndex, shown], ([index, count]) => {
    if (count > 0 && index >= count && rows.value[count - 1]) {
      selectedKey.value = rows.value[count - 1].key;
      confirming.value = null;
    }
  });

  async function tick(schedule: boolean) {
    const ticket = ++generation;
    if (schedule) window.clearTimeout(timer);
    try {
      const next = await resourcesSnapshot(sort.value, filter.value, 60);
      if (ticket !== generation) return;
      data.value = next;
      apply(next.rows);
    } catch (error) {
      notice.value = errorMessage(error);
    } finally {
      if (schedule && live.value) timer = window.setTimeout(() => void tick(true), rate.value);
    }
  }

  // While the user is moving through the list, rows keep their places (new ones join at the
  // end), so the one about to be picked doesn't slide away.
  function apply(next: ResourceRow[]) {
    if ((Date.now() < heldUntil || confirming.value) && rows.value.length) {
      const place = new Map(rows.value.map((row, index) => [row.key, index]));
      next = [...next].sort(
        (a, b) => (place.get(a.key) ?? Infinity) - (place.get(b.key) ?? Infinity)
      );
    }
    rows.value = next;
    if (!next.some((row) => row.key === selectedKey.value)) {
      selectedKey.value = next[0]?.key ?? '';
      confirming.value = null;
    }
  }

  function select(index: number) {
    const row = rows.value[index];
    if (!row) return;
    heldUntil = Date.now() + HOLD_MS;
    if (row.key !== selectedKey.value) confirming.value = null;
    selectedKey.value = row.key;
  }

  function move(delta: number) {
    const count = Math.min(rows.value.length, shown.value);
    if (count === 0) return;
    select((Math.min(selectedIndex.value, count - 1) + delta + count) % count);
  }

  function setSort(next: ResourceSort) {
    sort.value = next;
  }

  function cycleSort(backward: boolean) {
    const index = SORTS.findIndex((item) => item.id === sort.value);
    sort.value = SORTS[(index + (backward ? -1 : 1) + SORTS.length) % SORTS.length].id;
  }

  async function switchTo(row = current.value) {
    if (!row) return;
    try {
      if (!(await resourcesSwitch(row.pids))) {
        notice.value = `${row.name} has no windows to show.`;
      }
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  /** First press asks on the row; the second (or Enter) does it. */
  function askQuit(force: boolean) {
    const row = current.value;
    if (!row) return;
    if (force && row.protected) {
      notice.value = `${row.name} keeps the system running, so it can't be force quit.`;
      return;
    }
    const same = confirming.value?.key === row.key && confirming.value.force === force;
    if (same) void quit(row, force);
    else confirming.value = { key: row.key, force };
  }

  async function quit(row: ResourceRow, force: boolean) {
    confirming.value = null;
    actionsOpen.value = false;
    try {
      if (row.relaunch && !force) {
        await resourcesRelaunch(row.name, row.pids);
        notice.value = `Relaunched ${row.name}.`;
      } else {
        await resourcesQuit(row.name, row.pids, force);
        notice.value = `${force ? 'Force quit' : 'Quit'} ${row.name}.`;
      }
      heldUntil = 0;
      window.setTimeout(() => void tick(false), 400);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function reveal() {
    const row = current.value;
    if (!row?.path) return;
    try {
      await resourcesReveal(row.path);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      notice.value = `Copied the ${what}.`;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  const actions = computed<Action[]>(() => {
    const row = current.value;
    if (!row) return [];
    const list: Action[] = [
      { label: `Switch to ${row.name}`, keys: '↵', run: () => switchTo(row) },
    ];
    list.push(
      row.relaunch
        ? { label: `Relaunch ${row.name}`, keys: `${mod}⌫`, run: () => quit(row, false) }
        : { label: `Quit ${row.name}`, keys: `${mod}⌫`, run: () => quit(row, false) }
    );
    if (!row.protected) {
      list.push({ label: 'Force Quit', keys: `${alt}${mod}⌫`, run: () => quit(row, true) });
    }
    if (row.path) list.push({ label: `Show in ${finder}`, run: () => reveal() });
    list.push({ label: `Search the web for ${row.name}`, run: () => emit('search', row.name) });
    list.push({
      label: `Ask AI what ${row.name} is`,
      run: () =>
        emit(
          'ask',
          `What is "${row.name}" on my ${isMac ? 'Mac' : 'PC'}, and is it safe to quit?` +
            (row.path ? ` Its path is ${row.path}.` : '')
        ),
    });
    list.push({
      label: row.pids.length > 1 ? 'Copy process IDs' : 'Copy process ID',
      run: () => copy(row.pids.join(' '), row.pids.length > 1 ? 'process IDs' : 'process ID'),
    });
    if (row.path) list.push({ label: 'Copy path', run: () => copy(row.path, 'path') });
    if (row.command) list.push({ label: 'Copy command', run: () => copy(row.command, 'command') });
    const nextRate = RATES[(RATES.indexOf(rate.value) + 1) % RATES.length];
    list.push({
      label: `Refresh every ${nextRate / 1000} s (now ${rate.value / 1000} s)`,
      keepOpen: true,
      run: () => {
        rate.value = nextRate;
        void tick(true);
      },
    });
    return list;
  });

  function runAction(action: Action | undefined) {
    if (!action) return;
    if (!action.keepOpen) actionsOpen.value = false;
    void action.run();
  }

  /** Called by the bar for every key while System Resources is showing. */
  function onKey(event: KeyboardEvent) {
    const modifier = event.metaKey || event.ctrlKey;
    if (modifier && event.code === 'KeyK') {
      event.preventDefault();
      confirming.value = null;
      actionsOpen.value = !actionsOpen.value && actions.value.length > 0;
      actionIndex.value = 0;
      return;
    }
    if (actionsOpen.value) {
      if (event.key === 'Escape') {
        event.preventDefault();
        actionsOpen.value = false;
        return;
      } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const count = Math.min(actions.value.length, actionsShown.value);
        if (count === 0) return;
        actionIndex.value =
          (actionIndex.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
        return;
      } else if (event.key === 'Enter') {
        event.preventDefault();
        if (!event.repeat) runAction(actions.value[actionIndex.value]);
        return;
      }
      if (['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) return;
      // Any other key closes the menu and does what it does in the list.
      actionsOpen.value = false;
    }
    if (modifier && event.key === 'Backspace') {
      event.preventDefault();
      if (!event.repeat) askQuit(event.altKey);
      return;
    }
    if (confirming.value) {
      const pending = confirming.value;
      if (event.key === 'Enter') {
        event.preventDefault();
        const row = current.value;
        if (row && !event.repeat) void quit(row, pending.force);
        return;
      }
      confirming.value = null;
      if (event.key === 'Escape') {
        event.preventDefault();
        return;
      }
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      if (notice.value) notice.value = null;
      else if (filter.value) filter.value = '';
      else emit('exit');
      return;
    }
    if (event.key === 'Backspace' && !event.repeat && filter.value === '') {
      event.preventDefault();
      emit('exit');
      return;
    }
    if (event.key === 'Tab') {
      event.preventDefault();
      cycleSort(event.shiftKey);
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      move(event.key === 'ArrowDown' ? 1 : -1);
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (!event.repeat) void switchTo();
    }
  }

  function percent(value: number): string {
    return value < 10 ? `${value.toFixed(1)}%` : `${Math.round(value)}%`;
  }

  function uptime(seconds: number): string {
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    if (days) return `${days}d ${hours}h`;
    if (hours) return `${hours}h ${minutes}m`;
    return `${minutes}m`;
  }

  /** A minute of values as a filled sparkline in a 120×28 box, newest at the right. */
  /**
   * The last minute as a filled sparkline in a 120×28 box, now at the right edge, placed by
   * each point's age; a gap of more than a few seconds (the computer asleep) breaks the line.
   */
  function spark(points: ResourcePoint[], value: (point: ResourcePoint) => number, max: number) {
    const width = 120;
    const height = 28;
    const top = Math.max(max, 1e-9);
    const runs: string[][] = [];
    let previous: number | null = null;
    for (const point of points) {
      if (previous === null || previous - point.age > 3) runs.push([]);
      previous = point.age;
      const x = width - (Math.min(point.age, 60) / 60) * width;
      const y = height - 1 - Math.min(1, value(point) / top) * (height - 3);
      runs[runs.length - 1].push(`${x.toFixed(1)},${y.toFixed(1)}`);
    }
    return {
      line: runs.map((run) => `M${run.join(' L')}`).join(' '),
      area: runs
        .map((run) => {
          const first = run[0].split(',')[0];
          const last = run[run.length - 1].split(',')[0];
          return `M${first},${height} L${run.join(' L')} L${last},${height} Z`;
        })
        .join(' '),
    };
  }

  const tiles = computed(() => {
    const snapshot = data.value;
    if (!snapshot) return null;
    const history = snapshot.history;
    const traffic = (point: ResourcePoint) => point.down + point.up;
    const totals = snapshot.totals;
    return {
      cpu: {
        value: percent(totals.cpu),
        spark: spark(history, (p) => p.cpu, 100),
      },
      memory: {
        value: `${bytes(totals.memoryUsed)} of ${bytes(totals.memoryTotal, 0)}`,
        spark: spark(history, (p) => p.memory, 1),
      },
      network: {
        value: `↓ ${bytes(totals.netDown)}/s · ↑ ${bytes(totals.netUp)}/s`,
        spark: spark(history, traffic, Math.max(...history.map(traffic), 64 * 1024)),
      },
    };
  });

  /** The row's CPU bar: a full bar is one whole core, or the whole machine on Windows. */
  function cpuWidth(row: ResourceRow): string {
    return `${Math.min(100, Math.max(0, row.cpu ?? 0))}%`;
  }

  defineExpose({ onKey, start, stop });
</script>

<template>
  <div class="resources">
    <div class="input-row">
      <svg class="mark" viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.cpu" />
      <span class="chip scope-label">Resources</span>
      <input
        ref="inputEl"
        v-model="filter"
        class="query"
        type="text"
        placeholder="Filter apps and processes"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
      />
      <span
        class="res-live"
        :class="{ on: live }"
        :title="live ? `Live, every ${rate / 1000} s` : 'Paused'"
        aria-hidden="true"
      />
      <button
        type="button"
        class="chip"
        title="Sort (Tab)"
        @mousedown.prevent
        @click="cycleSort(false)"
      >
        {{ SORTS.find((item) => item.id === sort)?.label }}
      </button>
    </div>

    <div v-if="tiles" class="res-tiles">
      <button
        type="button"
        class="res-tile"
        :class="{ active: sort === 'cpu' }"
        @mousedown.prevent
        @click="setSort('cpu')"
      >
        <span class="res-tile-head">
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.cpu" />
          CPU
        </span>
        <span class="res-tile-value">{{ tiles.cpu.value }}</span>
        <svg
          class="res-spark cpu"
          viewBox="0 0 120 28"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <path class="area" :d="tiles.cpu.spark.area" />
          <path class="line" :d="tiles.cpu.spark.line" />
        </svg>
      </button>
      <button
        type="button"
        class="res-tile"
        :class="{ active: sort === 'memory' }"
        @mousedown.prevent
        @click="setSort('memory')"
      >
        <span class="res-tile-head">
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.memory" />
          Memory
        </span>
        <span class="res-tile-value">{{ tiles.memory.value }}</span>
        <svg
          class="res-spark memory"
          viewBox="0 0 120 28"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <path class="area" :d="tiles.memory.spark.area" />
          <path class="line" :d="tiles.memory.spark.line" />
        </svg>
      </button>
      <div class="res-tile">
        <span class="res-tile-head">
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.updown" />
          Network
        </span>
        <span class="res-tile-value">{{ tiles.network.value }}</span>
        <svg
          class="res-spark network"
          viewBox="0 0 120 28"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <path class="area" :d="tiles.network.spark.area" />
          <path class="line" :d="tiles.network.spark.line" />
        </svg>
      </div>
    </div>

    <p v-if="notice" class="notice">{{ notice }}</p>

    <ul
      v-if="actionsOpen"
      v-fit="(count: number) => (actionsShown = count)"
      class="results actions res-actions"
      role="listbox"
      aria-label="Actions"
    >
      <li
        v-for="(action, index) in actions"
        :key="action.label"
        role="option"
        :aria-selected="index === actionIndex"
        class="item"
        :class="{ selected: index === actionIndex }"
        @mousedown.prevent
        @click="runAction(action)"
        @mousemove="actionIndex = index"
      >
        <span class="label">{{ action.label }}</span>
        <span v-if="action.keys" class="hint">{{ action.keys }}</span>
      </li>
    </ul>

    <template v-else>
      <div class="res-columns" role="presentation">
        <button
          v-for="column in COLUMNS"
          :key="column.id"
          type="button"
          :class="[`res-col-${column.id}`, { active: sort === column.id }]"
          @mousedown.prevent
          @click="setSort(column.id)"
        >
          {{ column.label }}
        </button>
      </div>
      <p v-if="data && rows.length === 0" class="empty">
        {{ filter ? 'Nothing running matches.' : 'Reading processes…' }}
      </p>
      <ul
        v-else
        v-fit="(count: number) => (shown = count)"
        class="res-list"
        role="listbox"
        aria-label="Apps and processes"
      >
        <li
          v-for="(row, index) in rows"
          :key="row.key"
          role="option"
          :aria-selected="index === selectedIndex"
          class="res-row"
          :class="{
            selected: index === selectedIndex,
            confirming: confirming?.key === row.key,
            locked: row.locked,
          }"
          :title="
            row.locked
              ? `${row.name} runs as ${row.user || 'another user'}; its use needs administrator rights to read`
              : undefined
          "
          @mousedown.prevent
          @click="select(index)"
          @dblclick="switchTo(row)"
        >
          <span class="res-name">
            <RowIcon :icon="row.icon" :label="row.name" :size="20" />
            <span class="res-text">
              <span class="label">
                {{ row.name }}
                <span
                  v-if="row.pids.length > 1"
                  class="res-count"
                  :title="`${row.pids.length} processes`"
                  >{{ row.pids.length }}</span
                >
              </span>
              <span v-if="row.subtitle" class="res-sub">{{ row.subtitle }}</span>
            </span>
          </span>
          <span v-if="confirming?.key === row.key" class="res-confirm">
            {{
              row.relaunch && !confirming.force
                ? `Relaunch ${row.name}?`
                : `${confirming.force ? 'Force quit' : 'Quit'} ${row.name}?`
            }}
            <kbd class="kbd">↵</kbd> yes · <kbd class="kbd">Esc</kbd> no
          </span>
          <template v-else>
            <span class="res-cpu">
              <template v-if="row.cpu !== null">
                {{ percent(row.cpu) }}
                <span class="res-bar" aria-hidden="true"
                  ><span :style="{ width: cpuWidth(row) }"
                /></span>
              </template>
              <template v-else>—</template>
            </span>
            <span class="res-memory">{{ row.memory !== null ? bytes(row.memory) : '—' }}</span>
            <span class="res-disk">
              {{ row.disk === null ? '—' : row.disk ? `${bytes(row.disk)}/s` : '0' }}
            </span>
          </template>
        </li>
      </ul>
    </template>

    <footer class="footer">
      <span>
        <template v-if="actionsOpen">↵ run · Esc back</template>
        <template v-else>
          ↵ switch · {{ mod }}⌫ quit · {{ alt }}{{ mod }}⌫ force · Tab sort · {{ mod }}K actions
        </template>
      </span>
      <span v-if="data" :title="`${data.processes} processes`">
        {{ data.totals.diskTotal ? `${bytes(data.totals.diskFree)} free · ` : '' }}up
        {{ uptime(data.uptime) }}
      </span>
    </footer>
  </div>
</template>
