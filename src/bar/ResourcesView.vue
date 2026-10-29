<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue';
  import RowIcon from './RowIcon.vue';
  import { GLYPHS } from './glyphs';
  import { vTip } from '../tip';
  import { bytes } from '../format';
  import { gpuTitle } from './footerStats';
  import { isMac } from '../keys';
  import { useLater, usePoll } from '../poll';
  import KeyHints from '../components/KeyHints.vue';
  import ActionMenu from '../components/ActionMenu.vue';
  import { MENU_HINTS, isMenuKey, useActionMenu, type MenuAction } from '../components/actionMenu';
  import ResourceDetailPane from './ResourceDetail.vue';
  import ResourceDrill from './ResourceDrill.vue';
  import {
    errorMessage,
    resourcesClosed,
    resourcesCores,
    resourcesDetail,
    resourcesMemory,
    resourcesQuit,
    resourcesRelaunch,
    resourcesReveal,
    resourcesSnapshot,
    resourcesSwitch,
  } from '../api';
  import type {
    Battery,
    CoreHeat,
    MemoryBreakdown,
    ResourceDetail,
    ResourcePoint,
    ResourceRow,
    ResourceSnapshot,
    ResourceSort,
  } from '../types';

  // System Resources: the machine's CPU, memory, GPU and network over the last 15 minutes, and the
  // apps using them, live. Rows are apps (helpers counted in) or command-line programs; Enter
  // switches to one, ⌘⌫ quits it, ⌥⌘⌫ force quits it, each after an inline confirm.
  const emit = defineEmits<{ exit: []; search: [text: string]; network: [] }>();
  /** Text to filter by on opening (a process ID, from a port's row). */
  const props = defineProps<{ initialFilter?: string }>();

  const SORTS: { id: ResourceSort; label: string }[] = [
    { id: 'cpu', label: 'CPU' },
    { id: 'memory', label: 'Memory' },
    // The third column: per-process GPU on Windows, which counts it; disk I/O on macOS,
    // where per-process GPU needs root.
    isMac ? { id: 'disk', label: 'Disk' } : { id: 'gpu', label: 'GPU' },
    { id: 'name', label: 'Name' },
  ];
  const COLUMNS = [SORTS[3], SORTS[0], SORTS[1], SORTS[2]];
  const RATES = [500, 1000, 2000];
  const RATE_KEY = 'zephyr.resources.rate';
  /** The refresh rate picked in ⌘K, remembered in this window's storage. */
  function savedRate(): number {
    try {
      const saved = Number(localStorage.getItem(RATE_KEY));
      return RATES.includes(saved) ? saved : 1000;
    } catch {
      return 1000;
    }
  }
  /** After a key or the pointer moves the selection, the order holds this long. */
  const HOLD_MS = 2000;
  /** The order changes at most this often, by values smoothed over about this many seconds. */
  const ORDER_MS = 3000;
  const SMOOTH_S = 4;
  /** Every row, for the scrolling list. */
  const ROWS_WANTED = 500;
  /** Seconds of history the sparklines span (stats.rs keeps this much). */
  const WINDOW = 900;

  const LIST_HINTS = [
    { keys: 'enter', label: 'switch' },
    { keys: isMac ? 'mod+backspace' : 'delete', label: 'quit' },

    { keys: 'shift+space', label: 'info' },
    { keys: 'tab', label: 'sort' },
    { keys: 'mod+k', label: 'actions' },
  ];
  const CONFIRM_HINTS = [
    { keys: 'enter', label: 'yes' },
    { keys: 'esc', label: 'no' },
  ];
  const finder = isMac ? 'Finder' : 'Explorer';

  const inputEl = ref<HTMLInputElement | null>(null);
  const filter = ref(props.initialFilter ?? '');
  const sort = ref<ResourceSort>('cpu');
  // Replaced whole each tick, never edited, so it needs no deep tracking.
  const data = shallowRef<ResourceSnapshot | null>(null);
  const rows = ref<ResourceRow[]>([]);
  const selectedKey = ref('');
  const notice = ref<string | null>(null);
  /** The quit waiting for a second press, on the selected row. */
  const confirming = ref<{ key: string; force: boolean } | null>(null);
  const menu = useActionMenu(() => actions.value);
  const { open: actionsOpen, index: actionIndex } = menu;
  const rate = ref(savedRate());
  /** Show info for the selected row, and the tiles' drill-ins in place of the list. */
  const detailOpen = ref(false);
  const detail = ref<ResourceDetail | null>(null);
  const drill = ref<'cpu' | 'memory' | null>(null);
  const heat = ref<CoreHeat | null>(null);
  const memoryParts = ref<MemoryBreakdown | null>(null);
  const live = ref(false);

  /** How long the last tick took, start to finish, for the footer's tooltip. */
  const tickMs = ref(0);

  let generation = 0;
  let heldUntil = 0;
  /** When the order last changed, and each row's smoothed value (see `apply`). */
  let rankedAt = 0;
  let smoothedAt = 0;
  const smoothed = new Map<string, number>();
  const pointerIn = ref(false);
  const later = useLater();
  // Ticks at the picked rate while live.
  const poll = usePoll(tick, rate, live);

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

  // Show info follows the selection.
  watch(selectedKey, () => {
    detail.value = null;
  });

  watch([filter, sort], () => {
    heldUntil = 0;
    rankedAt = 0;
    smoothedAt = 0;
    smoothed.clear();
    confirming.value = null;
    void tick();
  });

  /** Ticks while the bar shows; the bar stops it when it hides. */
  function start() {
    if (live.value) poll.restart();
    else live.value = true;
  }

  /** The backend lets the process table go once nothing has asked for it in a while. */
  function stop() {
    live.value = false;
    void resourcesClosed().catch(() => undefined);
  }

  // The list scrolls; the selection stays in sight as keys move it.
  watch(selectedIndex, async (index) => {
    await nextTick();
    document.getElementById(`res-row-${index}`)?.scrollIntoView({ block: 'nearest' });
  });

  async function tick() {
    const ticket = ++generation;
    try {
      const started = performance.now();
      const next = await resourcesSnapshot(sort.value, filter.value, ROWS_WANTED);
      if (ticket !== generation) return;
      data.value = next;
      apply(next.rows);
      // Only what's on screen is asked for.
      const row = current.value;
      if (detailOpen.value && row) {
        detail.value = await resourcesDetail(row.key, row.pids).catch(() => null);
      }
      if (drill.value === 'cpu') heat.value = await resourcesCores().catch(() => null);
      if (drill.value === 'memory') {
        memoryParts.value = await resourcesMemory().catch(() => null);
      }
      tickMs.value = Math.round(performance.now() - started);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  /** The value the list sorts by, or null for Name (and when it can't be read). */
  function metric(row: ResourceRow): number | null {
    switch (sort.value) {
      case 'memory':
        return row.memory;
      case 'disk':
        return row.disk;
      case 'gpu':
        return row.gpu ?? null;
      case 'name':
        return null;
      default:
        return row.cpu;
    }
  }

  /** Eases each row's value toward its new reading, so a one-tick spike doesn't reorder. */
  function smooth(next: ResourceRow[]) {
    const now = Date.now();
    const weight = smoothedAt ? 1 - Math.exp(-(now - smoothedAt) / 1000 / SMOOTH_S) : 1;
    smoothedAt = now;
    const before = new Map(smoothed);
    smoothed.clear();
    for (const row of next) {
      const value = metric(row);
      if (value === null) continue;
      const last = before.get(row.key);
      smoothed.set(row.key, last === undefined ? value : last + (value - last) * weight);
    }
  }

  /** Apps first, then background processes; each by name or by its smoothed value. */
  function ranked(next: ResourceRow[]): ResourceRow[] {
    const byName = (a: ResourceRow, b: ResourceRow) =>
      a.name.toLowerCase().localeCompare(b.name.toLowerCase());
    return [...next].sort((a, b) => {
      if (a.windowed !== b.windowed) return a.windowed ? -1 : 1;
      if (sort.value === 'name') return byName(a, b);
      const left = smoothed.get(a.key);
      const right = smoothed.get(b.key);
      if (left === undefined || right === undefined) {
        return left === right ? byName(a, b) : left === undefined ? 1 : -1;
      }
      return right - left || byName(a, b);
    });
  }

  // The order changes every few seconds at most, by values smoothed over a few seconds. In
  // between, and while the pointer is over the list or the keys are moving through it, rows
  // keep their places (new ones join the end of their group) so nothing slides away.
  function apply(next: ResourceRow[]) {
    smooth(next);
    const now = Date.now();
    const holding =
      (now < heldUntil || confirming.value !== null || pointerIn.value) && rows.value.length > 0;
    if (!holding && now - rankedAt >= ORDER_MS) {
      next = ranked(next);
      rankedAt = now;
    } else {
      const place = new Map(rows.value.map((row, index) => [row.key, index]));
      const arrival = new Map(next.map((row, index) => [row.key, rows.value.length + index]));
      const at = (row: ResourceRow) => place.get(row.key) ?? arrival.get(row.key) ?? 0;
      next = [...next].sort((a, b) =>
        a.windowed !== b.windowed ? (a.windowed ? -1 : 1) : at(a) - at(b)
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

  /** "Apps" above the first app row and "Background" above the first other one, when both. */
  const hasApps = computed(() => rows.value.some((row) => row.windowed));
  const hasBackground = computed(() => rows.value.some((row) => !row.windowed));
  function groupLabel(index: number): string {
    if (!hasApps.value || !hasBackground.value) return '';
    const row = rows.value[index];
    if (index > 0 && rows.value[index - 1].windowed === row.windowed) return '';
    return row.windowed ? 'Apps' : 'Background processes';
  }

  function move(delta: number) {
    const count = rows.value.length;
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
      // A process with no window shows its info instead.
      if (!(await resourcesSwitch(row.pids))) openDetail();
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
      later(tick, 400);
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

  /** The list leaves command lines out, so this asks for the row's. */
  async function copyCommand(row: ResourceRow) {
    const found =
      detail.value?.key === row.key ? detail.value : await resourcesDetail(row.key, row.pids);
    if (found?.command) await copy(found.command, 'command');
    else notice.value = `${row.name}'s command line needs administrator rights to read.`;
  }

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      notice.value = `Copied the ${what}.`;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  const QUIT_KEYS = isMac ? 'mod+backspace' : 'delete';
  const FORCE_KEYS = isMac ? 'mod+alt+backspace' : 'shift+delete';
  /** Finder and the Dock come back by themselves on a Mac; Explorer is restarted. */
  const RELAUNCH = isMac ? 'Relaunch' : 'Restart';

  const actions = computed<MenuAction[]>(() => {
    const row = current.value;
    if (!row) return [];
    const noPath = row.locked ? 'Needs administrator rights' : 'Not known';
    const list: MenuAction[] = [
      { label: `Switch to ${row.name}`, keys: 'enter', run: () => switchTo(row) },
      { label: 'Show info', keys: 'shift+space', run: () => openDetail() },
      row.relaunch
        ? { label: `${RELAUNCH} ${row.name}`, keys: QUIT_KEYS, run: () => quit(row, false) }
        : { label: `Quit ${row.name}`, keys: QUIT_KEYS, run: () => quit(row, false) },
      {
        label: 'Force quit',
        keys: FORCE_KEYS,
        disabled: row.protected ? 'Keeps the system running' : undefined,
        run: () => quit(row, true),
      },
      {
        label: `Show in ${finder}`,
        disabled: row.path ? undefined : noPath,
        run: () => reveal(),
      },
      { label: `Search the web for ${row.name}`, run: () => emit('search', row.name) },
      {
        label: row.pids.length > 1 ? 'Copy process IDs' : 'Copy process ID',
        run: () => copy(row.pids.join(' '), row.pids.length > 1 ? 'process IDs' : 'process ID'),
      },
      {
        label: 'Copy path',
        disabled: row.path ? undefined : noPath,
        run: () => copy(row.path, 'path'),
      },
      {
        label: 'Copy command',
        disabled: row.locked ? noPath : undefined,
        run: () => copyCommand(row),
      },
    ];
    const nextRate = RATES[(RATES.indexOf(rate.value) + 1) % RATES.length];
    list.push({
      label: `Refresh every ${nextRate / 1000} s (now ${rate.value / 1000} s)`,
      keepOpen: true,
      run: () => {
        rate.value = nextRate;
        try {
          localStorage.setItem(RATE_KEY, String(nextRate));
        } catch {
          // Not remembered this time; it still applies now.
        }
        poll.restart();
      },
    });
    return list;
  });

  function openDetail() {
    if (!current.value) return;
    drill.value = null;
    detailOpen.value = true;
    detail.value = null;
    void tick();
  }

  /** The CPU or Memory tile: its drill-in in place of the list, or back to the list. */
  function toggleDrill(kind: 'cpu' | 'memory') {
    detailOpen.value = false;
    drill.value = drill.value === kind ? null : kind;
    if (drill.value) void tick();
  }

  /** Called by the bar for every key while System Resources is showing. */
  function onKey(event: KeyboardEvent) {
    const modifier = event.metaKey || event.ctrlKey;
    // Opening the menu backs out of a quit waiting for its second press.
    if (isMenuKey(event)) confirming.value = null;
    if (menu.onKey(event)) return;
    if (modifier && event.key === 'Backspace' && isMac) {
      event.preventDefault();
      if (!event.repeat) askQuit(event.altKey);
      return;
    }
    // Windows: Delete ends the task, as in Task Manager, unless it would delete typed text.
    const input = inputEl.value;
    const caretAtEnd = !input || input.selectionStart === filter.value.length;
    if (!isMac && event.key === 'Delete' && !modifier && caretAtEnd) {
      event.preventDefault();
      if (!event.repeat) askQuit(event.shiftKey);
      return;
    }
    if (event.key === ' ' && event.shiftKey && !modifier && !event.altKey) {
      event.preventDefault();
      if (event.repeat) return;
      if (detailOpen.value) detailOpen.value = false;
      else openDetail();
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
      if (detailOpen.value) detailOpen.value = false;
      else if (drill.value) drill.value = null;
      else if (notice.value) notice.value = null;
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

  /** "54 °C": hardware temperatures are always Celsius, whatever unit the weather uses. */
  function degrees(value: number | null | undefined): string {
    return value == null ? '' : `${Math.round(value)} °C`;
  }

  function percent(value: number): string {
    return value < 10 ? `${value.toFixed(1)}%` : `${Math.round(value)}%`;
  }

  /** "84%, 3:12 left", "Charging 61%", "Full". */
  function batteryText(battery: Battery): string {
    const percentText = `${Math.round(battery.percent)}%`;
    const left = battery.secondsLeft
      ? `${Math.floor(battery.secondsLeft / 3600)}:${String(Math.floor((battery.secondsLeft % 3600) / 60)).padStart(2, '0')}`
      : '';
    if (battery.state === 'full') return `Battery full`;
    if (battery.state === 'charging') {
      return `Charging ${percentText}${left ? `, ${left} to full` : ''}`;
    }
    return `Battery ${percentText}${left ? `, ${left} left` : ''}`;
  }

  function uptime(seconds: number): string {
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    if (days) return `${days}d ${hours}h`;
    if (hours) return `${hours}h ${minutes}m`;
    return `${minutes}m`;
  }

  /**
   * The last 15 minutes as a filled sparkline in a 120×28 box, now at the right edge, placed by
   * each point's age; a gap of more than a few seconds (the computer asleep) breaks the line.
   */
  function spark(points: ResourcePoint[], value: (point: ResourcePoint) => number, max: number) {
    const width = 120;
    const height = 28;
    const top = Math.max(max, 1e-9);
    const runs: string[][] = [];
    let previous: number | null = null;
    for (const point of points) {
      // Points come a stretch apart (7.5 s); a wider gap is the computer asleep.
      if (previous === null || previous - point.age > 12) runs.push([]);
      previous = point.age;
      const x = width - (Math.min(point.age, WINDOW) / WINDOW) * width;
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
        temp: degrees(totals.cpuTemp),
        spark: spark(history, (p) => p.cpu, 100),
      },
      memory: {
        value: `${bytes(totals.memoryUsed)} of ${bytes(totals.memoryTotal, 0)}`,
        spark: spark(history, (p) => p.memory, 1),
      },
      gpu: totals.gpu
        ? {
            value: totals.gpu.memoryUsed
              ? `${percent(totals.gpu.usage)} · ${bytes(totals.gpu.memoryUsed)}`
              : percent(totals.gpu.usage),
            title: gpuTitle(
              totals.gpu.name,
              totals.gpu.usage,
              totals.gpu.memoryUsed,
              totals.gpu.memoryTotal
            ),
            spark: spark(history, (p) => p.gpu, 100),
            temp: degrees(totals.gpuTemp),
          }
        : null,
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
        role="combobox"
        aria-label="Filter apps and processes"
        aria-autocomplete="list"
        :aria-controls="actionsOpen ? 'res-actions' : 'res-list'"
        :aria-expanded="actionsOpen || rows.length > 0"
        :aria-activedescendant="
          actionsOpen
            ? `res-actions-${actionIndex}`
            : rows.length && !drill && !detailOpen
              ? `res-row-${selectedIndex}`
              : undefined
        "
        placeholder="Filter apps and processes"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
      />
      <span
        class="res-live"
        :class="{ on: live }"
        v-tip="live ? `Live, every ${rate / 1000} s` : 'Paused'"
        aria-hidden="true"
      />
      <button
        type="button"
        class="chip"
        v-tip="'Sort (Tab)'"
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
        :class="{ active: drill === 'cpu' }"
        v-tip="drill === 'cpu' ? 'Back to the list (Esc)' : 'Each processor'"
        @mousedown.prevent
        @click="toggleDrill('cpu')"
      >
        <span class="res-tile-head">
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.cpu" />
          CPU
          <span v-if="tiles.cpu.temp" class="res-temp" v-tip="'CPU temperature'">{{
            tiles.cpu.temp
          }}</span>
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
        :class="{ active: drill === 'memory' }"
        v-tip="drill === 'memory' ? 'Back to the list (Esc)' : 'Where the memory is'"
        @mousedown.prevent
        @click="toggleDrill('memory')"
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
      <div v-if="tiles.gpu" class="res-tile" v-tip="tiles.gpu.title">
        <span class="res-tile-head">
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.gpu" />
          GPU
          <span v-if="tiles.gpu.temp" class="res-temp" v-tip="'GPU temperature'">{{
            tiles.gpu.temp
          }}</span>
        </span>
        <span class="res-tile-value">{{ tiles.gpu.value }}</span>
        <svg
          class="res-spark gpu"
          viewBox="0 0 120 28"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <path class="area" :d="tiles.gpu.spark.area" />
          <path class="line" :d="tiles.gpu.spark.line" />
        </svg>
      </div>
      <button
        type="button"
        class="res-tile"
        v-tip="'Open Network'"
        @mousedown.prevent
        @click="emit('network')"
      >
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
      </button>
    </div>

    <p v-if="notice" class="notice">{{ notice }}</p>

    <ActionMenu
      v-if="actionsOpen"
      id="res-actions"
      class="res-actions"
      :actions="actions"
      :index="actionIndex"
      @run="menu.run"
      @hover="menu.hover"
      @fit="menu.shown.value = $event"
    />

    <ResourceDrill v-else-if="drill" :kind="drill" :heat="heat" :memory="memoryParts" />

    <ResourceDetailPane
      v-else-if="detailOpen && current"
      :row="current"
      :detail="detail"
      @switch="switchTo(current)"
      @quit="askQuit"
      @reveal="reveal"
      @close="detailOpen = false"
    />

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
        id="res-list"
        class="res-list"
        role="listbox"
        aria-label="Apps and processes"
        @mouseenter="pointerIn = true"
        @mouseleave="pointerIn = false"
      >
        <template v-for="(row, index) in rows" :key="row.key">
          <li v-if="groupLabel(index)" class="res-group" role="presentation">
            {{ groupLabel(index) }}
          </li>
          <li
            :id="`res-row-${index}`"
            role="option"
            :aria-selected="index === selectedIndex"
            class="res-row"
            :class="{
              selected: index === selectedIndex,
              confirming: confirming?.key === row.key,
              locked: row.locked,
            }"
            v-tip="
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
                    v-tip="`${row.pids.length} processes`"
                    >{{ row.pids.length }}</span
                  >
                </span>
                <span v-if="row.subtitle" class="res-sub">{{ row.subtitle }}</span>
              </span>
            </span>
            <span v-if="confirming?.key === row.key" class="res-confirm">
              {{
                row.relaunch && !confirming.force
                  ? `${RELAUNCH} ${row.name}?`
                  : `${confirming.force ? 'Force quit' : 'Quit'} ${row.name}?`
              }}
              <KeyHints :hints="CONFIRM_HINTS" />
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
              <span v-if="isMac" class="res-disk">
                {{ row.disk === null ? '—' : row.disk ? `${bytes(row.disk)}/s` : '0' }}
              </span>
              <span v-else class="res-disk res-gpu">
                {{
                  row.gpu === null || row.gpu === undefined ? '—' : row.gpu ? percent(row.gpu) : '0'
                }}
              </span>
            </template>
          </li>
        </template>
      </ul>
    </template>

    <footer class="footer">
      <KeyHints :hints="actionsOpen ? MENU_HINTS : LIST_HINTS" />
      <span
        v-if="data"
        v-tip="
          `${data.processes} processes
Each refresh takes ${tickMs} ms`
        "
      >
        <template v-if="data.battery">{{ batteryText(data.battery) }} · </template>
        {{ data.totals.diskTotal ? `${bytes(data.totals.diskFree)} free` : '' }}
        <template v-if="!data.battery">· up {{ uptime(data.uptime) }}</template>
      </span>
    </footer>
  </div>
</template>
