<script setup lang="ts">
  import { computed, onMounted, onUnmounted, ref } from 'vue';
  import { GLYPHS } from './glyphs';
  import { vTip } from '../tip';
  import { bytes, elapsed } from '../format';
  import { usePoll } from '../poll';
  import KeyHints from '../components/KeyHints.vue';
  import BackButton from './BackButton.vue';
  import ActionMenu from '../components/ActionMenu.vue';
  import SpeedPanel from '../components/SpeedPanel.vue';
  import { MENU_HINTS, useActionMenu, type MenuAction } from '../components/actionMenu';
  import { errorMessage, networkAddresses, networkHistory, networkPublicIp, openUrl } from '../api';
  import type {
    HopStats,
    NetworkAddress,
    NetworkBucket,
    NetworkHistory,
    SpeedResult,
  } from '../types';
  import { GRADE_NAMES, lastTestLine, lossText, ms, speedLine } from './network';
  import { useSpeedTest } from '../speedTest';

  // The Network view: how the connection has been over the last 15 minutes. A verdict, this
  // computer's addresses, a chart of latency (median and p95), loss and throughput, and the
  // router, the internet and DNS side by side. The speed test runs here too.
  const props = defineProps<{ lastSpeed: SpeedResult | null }>();
  const emit = defineEmits<{ exit: [] }>();

  const LIST_HINTS = [
    { keys: 'mod+k', label: 'actions' },
    { keys: 'esc', label: 'back' },
  ];
  const TEST_HINTS = [{ keys: 'esc', label: 'cancel' }];

  const data = ref<NetworkHistory | null>(null);
  const addresses = ref<NetworkAddress[]>([]);
  const publicIp = ref<string | null>(null);
  const notice = ref<string | null>(null);
  const menu = useActionMenu(() => actions.value);
  const { open: actionsOpen, index: actionIndex } = menu;
  const live = ref(false);

  /** The speed test: running with its latest reading, or finished with its result. */
  const speed = useSpeedTest();
  const { running: testRunning, testing, result } = speed;

  // The history every second and the public address every 30, while live.
  const history = usePoll(tick, 1000, live);
  const publicAddress = usePoll(loadPublic, 30000, live);

  onMounted(start);
  onUnmounted(stop);

  /** Refreshes while the bar shows; the bar stops it when it hides. */
  function start() {
    void loadAddresses();
    if (!live.value) {
      live.value = true;
      return;
    }
    history.restart();
    publicAddress.restart();
  }

  function stop() {
    live.value = false;
  }

  async function tick() {
    try {
      data.value = await networkHistory();
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function loadAddresses() {
    addresses.value = await networkAddresses().catch(() => []);
  }

  async function loadPublic() {
    publicIp.value = (await networkPublicIp().catch(() => null)) ?? publicIp.value;
  }

  const summary = computed(() => data.value?.summary ?? null);

  const tone = computed(() => {
    const now = summary.value;
    if (!now || !now.watching || now.median === null) return '';
    return now.offline ? 'poor' : now.grade;
  });
  const pill = computed(() => {
    const now = summary.value;
    if (!now || !now.watching) return 'Off';
    if (now.offline) return 'Offline';
    return now.median === null ? '…' : GRADE_NAMES[now.grade];
  });
  const verdict = computed(() => {
    const now = summary.value;
    if (!now) return 'Starting…';
    if (!now.watching) return 'Not watching the connection. Turn it on in Settings › Network.';
    return now.verdict;
  });

  /** The chips: the main local address, the public one, then the rest. */
  const chips = computed(() => {
    const list: { label: string; value: string }[] = [];
    const [first, ...rest] = addresses.value;
    if (first) list.push({ label: first.detail.split(' · ')[0] || 'Local', value: first.value });
    if (publicIp.value) list.push({ label: 'Public', value: publicIp.value });
    for (const address of rest) {
      list.push({ label: address.detail.replace(' · IPv4', ''), value: address.value });
    }
    return list;
  });
  const chipsShown = computed(() => chips.value.slice(0, 3));

  // The chart: latency over the top, throughput in a strip beneath, 180 stretches of five
  // seconds across, now at the right.
  const W = 664;
  const H = 196;
  const PL = 8;
  const PR = 54;
  const LT = 12;
  const LH = 112;
  const TT = 146;
  const TH = 32;
  const IW = W - PL - PR;
  const MAX_MS = 100;

  const buckets = computed<NetworkBucket[]>(() => data.value?.buckets ?? []);
  const bw = computed(() => IW / Math.max(1, buckets.value.length));
  const x = (index: number) => PL + index * bw.value;
  const cx = (index: number) => x(index) + bw.value / 2;
  const y = (value: number) => LT + LH - (Math.min(value, MAX_MS) / MAX_MS) * LH;

  const grid = [0, 50, 100].map((value) => ({ value, y: y(value) }));

  type Point = [number, number];

  /** A smooth curve through `points` (Catmull-Rom as cubic Béziers), kept inside the
   *  latency area so it never overshoots past 0 or the top. */
  function smooth(points: Point[], move = true): string {
    if (points.length === 0) return '';
    const clamp = (value: number) => Math.min(LT + LH, Math.max(LT, value));
    const f = (value: number) => value.toFixed(1);
    let path = `${move ? 'M' : 'L'}${f(points[0][0])} ${f(points[0][1])}`;
    for (let i = 0; i < points.length - 1; i++) {
      const [x0, y0] = points[Math.max(0, i - 1)];
      const [x1, y1] = points[i];
      const [x2, y2] = points[i + 1];
      const [x3, y3] = points[Math.min(points.length - 1, i + 2)];
      const c1: Point = [x1 + (x2 - x0) / 6, clamp(y1 + (y2 - y0) / 6)];
      const c2: Point = [x2 - (x3 - x1) / 6, clamp(y2 - (y3 - y1) / 6)];
      path += `C${f(c1[0])} ${f(c1[1])} ${f(c2[0])} ${f(c2[1])} ${f(x2)} ${f(y2)}`;
    }
    return path;
  }

  const chart = computed(() => {
    const list = buckets.value;
    // Runs of measured stretches; a gap (asleep, offline) breaks the line.
    const runs: { median: Point[]; p95: Point[] }[] = [];
    let run: { median: Point[]; p95: Point[] } | null = null;
    list.forEach((bucket, index) => {
      if (bucket.median === null) {
        run = null;
        return;
      }
      if (!run) {
        run = { median: [], p95: [] };
        runs.push(run);
      }
      const at = cx(index);
      run.median.push([at, y(bucket.median)]);
      run.p95.push([at, y(bucket.p95 ?? bucket.median)]);
    });
    const base = (LT + LH).toFixed(1);
    let line = '';
    let area = '';
    let band = '';
    for (const { median, p95 } of runs) {
      const curve = smooth(median);
      line += curve;
      const first = median[0][0].toFixed(1);
      const last = median[median.length - 1][0].toFixed(1);
      area += `M${first} ${base}${smooth(median, false)}L${last} ${base}Z`;
      band += `${smooth(p95)}${smooth([...median].reverse(), false)}Z`;
    }
    const lastRun = runs[runs.length - 1];
    const tail = lastRun?.median[lastRun.median.length - 1];
    // The latest reading, as a dot, when it's the newest stretch.
    const now = tail && list[list.length - 1]?.median !== null ? { x: tail[0], y: tail[1] } : null;
    const ticks = list.flatMap((bucket, index) =>
      bucket.lost ? [{ x: x(index), drop: bucket.sent > 0 && bucket.lost * 2 >= bucket.sent }] : []
    );
    // Asleep, offline, a new network and speed tests, as columns and lines.
    const gaps = list.flatMap((bucket, index) =>
      bucket.flags & (1 | 16) && !bucket.sent ? [{ x: x(index) }] : []
    );
    const marks = list.flatMap((bucket, index) =>
      bucket.flags & 2 ? [{ x: cx(index), kind: 'change' }] : []
    );
    const tests: number[] = [];
    list.forEach((bucket, index) => {
      if (bucket.flags & 8 && !(list[index - 1]?.flags & 8)) tests.push(cx(index));
    });
    return { line, area, band, now, ticks, gaps, marks, tests };
  });

  const strip = computed(() => {
    const list = buckets.value;
    const top = Math.max(64 * 1024, ...list.map((bucket) => Math.max(bucket.down, bucket.up)));
    const mid = TT + TH * 0.6;
    const downY = (value: number) => mid - (value / top) * (TH * 0.6);
    const upY = (value: number) => mid + (value / top) * (TH * 0.4);
    let down = `M${PL} ${mid}`;
    let up = `M${PL} ${mid}`;
    list.forEach((bucket, index) => {
      down += `L${cx(index).toFixed(1)} ${downY(bucket.down).toFixed(1)}`;
      up += `L${cx(index).toFixed(1)} ${upY(bucket.up).toFixed(1)}`;
    });
    down += `L${W - PR} ${mid}Z`;
    up += `L${W - PR} ${mid}Z`;
    return { down, up, mid, top };
  });

  const used = computed(() => {
    const now = data.value;
    if (!now) return '';
    return `Used ↓ ${bytes(now.usedDown)}  ↑ ${bytes(now.usedUp)}  ·  peak ${bytes(Math.max(now.peakDown, now.peakUp))}/s`;
  });

  // Hovering the chart reads out one stretch.
  const svgEl = ref<SVGSVGElement | null>(null);
  const hovered = ref(-1);
  function hoverChart(event: MouseEvent) {
    const svg = svgEl.value;
    if (!svg) return;
    const rect = svg.getBoundingClientRect();
    const scale = W / rect.width;
    const px = (event.clientX - rect.left) * scale;
    const index = Math.floor((px - PL) / bw.value);
    hovered.value = index >= 0 && index < buckets.value.length ? index : -1;
  }
  const tip = computed(() => {
    const index = hovered.value;
    const bucket = buckets.value[index];
    if (!bucket) return null;
    const seconds = (buckets.value.length - index) * 5;
    const parts = [`${elapsed(seconds)} ago`];
    if (bucket.median === null) {
      parts.push(bucket.flags & 16 ? 'offline' : bucket.sent ? 'no replies' : 'not measured');
    } else {
      parts.push(`${ms(bucket.median)} ms (p95 ${ms(bucket.p95)})`);
    }
    if (bucket.lost) parts.push(`${bucket.lost}/${bucket.sent} lost`);
    parts.push(`↓ ${bytes(bucket.down)}/s ↑ ${bytes(bucket.up)}/s`);
    if (bucket.flags & 2) parts.push('network changed');
    if (bucket.flags & 8) parts.push('speed test');
    const at = cx(index);
    // Kept inside the chart, as a share of its width.
    const left = Math.min(Math.max(at / W, 0.16), 0.84) * 100;
    return { text: parts.join(' · '), left: `${left}%`, x: at };
  });

  /** Slow answers: amber past 100 ms, red past 300. */
  function slowness(value: number | null): string {
    return (value ?? 0) >= 300 ? 'bad' : (value ?? 0) >= 100 ? 'warn' : '';
  }

  const hops = computed(() => {
    const now = data.value;
    if (!now) return [];
    const row = (name: string, where: string, stats: HopStats, link = '') => ({
      name,
      where,
      link,
      now: stats.lastLost ? 'lost' : stats.last !== null ? `${ms(stats.last)} ms` : '—',
      nowTone: stats.lastLost ? 'bad' : slowness(stats.last),
      median: stats.sent ? `${ms(stats.median)} ms` : '—',
      p95: stats.sent ? `${ms(stats.p95)} ms` : '—',
      jitter: stats.sent ? `${ms(stats.jitter)} ms` : '—',
      jitterTone: (stats.jitter ?? 0) >= 30 ? 'bad' : (stats.jitter ?? 0) >= 10 ? 'warn' : '',
      loss: stats.sent ? `${lossText(stats.loss)}%` : '—',
      lossTone: stats.loss >= 0.02 ? 'bad' : stats.loss >= 0.005 ? 'warn' : '',
      slowTone: slowness(stats.p95),
      medianTone: slowness(stats.median),
    });
    return [
      // The router's own address opens its sign-in page.
      row(
        'Router',
        now.summary.gateway ?? 'none',
        now.router,
        now.summary.gateway ? `http://${now.summary.gateway}` : ''
      ),
      // A fixed, always-up host (Cloudflare) stands for the internet; DNS settings don't touch it.
      row('Internet', 'ping 1.1.1.1', now.internet),
      row('DNS', now.summary.dns ? `${now.summary.dns} · lookup` : 'none', now.dnsHop),
    ];
  });

  async function runSpeedTest() {
    if (testRunning.value) return;
    actionsOpen.value = false;
    notice.value = null;
    const error = await speed.run();
    if (error) notice.value = error;
  }

  const cancelSpeedTest = speed.cancel;

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      notice.value = `Copied ${what}.`;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  /** Plain text to paste to an ISP or into a chat. */
  function summaryText(): string {
    const now = data.value;
    if (!now) return '';
    const hop = (name: string, stats: HopStats) =>
      `${name}: median ${ms(stats.median)} ms, p95 ${ms(stats.p95)} ms, jitter ${ms(stats.jitter)} ms, ${lossText(stats.loss)}% loss`;
    const lines = [
      `Connection, last 15 minutes (${new Date().toLocaleString()})`,
      now.summary.verdict,
      hop(`Internet (1.1.1.1)`, now.internet),
      hop(`Router (${now.summary.gateway ?? 'none'})`, now.router),
      hop(`DNS (${now.summary.dns ?? 'none'})`, now.dnsHop),
    ];
    const last = result.value ?? props.lastSpeed;
    if (last) lines.push(`Speed test: ${speedLine(last)}`);
    return lines.join('\n');
  }

  const actions = computed<MenuAction[]>(() => {
    const list: MenuAction[] = [
      testRunning.value
        ? { label: 'Cancel speed test', run: cancelSpeedTest }
        : { label: 'Run speed test', run: runSpeedTest },
    ];
    for (const chip of chips.value) {
      list.push({
        label: `Copy ${chip.label} IP (${chip.value})`,
        run: () => copy(chip.value, chip.value),
      });
    }
    list.push({ label: 'Copy summary', run: () => copy(summaryText(), 'the summary') });
    const gateway = summary.value?.gateway;
    if (gateway) {
      list.push({
        label: `Open router page (${gateway})`,
        run: () => openRouter(`http://${gateway}`),
      });
    }
    return list;
  });

  /** The router's sign-in page, in the default browser. */
  function openRouter(url: string) {
    void openUrl(url).catch((error) => (notice.value = errorMessage(error)));
  }

  /** Called by the bar for every key while the Network view shows. */
  function onKey(event: KeyboardEvent) {
    if (menu.onKey(event)) return;
    if (event.key === 'Escape' || (event.key === 'Backspace' && !event.repeat)) {
      event.preventDefault();
      if (testRunning.value) cancelSpeedTest();
      else if (notice.value) notice.value = null;
      else if (result.value) result.value = null;
      else emit('exit');
      return;
    }
    // Nothing here takes typing; keep the keys from reaching anything else.
    if (event.key === 'Tab' || event.key === 'Enter') event.preventDefault();
  }

  defineExpose({ onKey, start, stop, runSpeedTest });
</script>

<template>
  <div class="network">
    <div class="input-row">
      <BackButton @back="emit('exit')" />
      <span class="chip scope-label">Network</span>
      <span class="net-title">Last 15 minutes</span>
      <span
        class="res-live"
        :class="{ on: live && summary?.watching }"
        v-tip="summary?.watching ? 'Live, every second' : 'Not watching'"
        aria-hidden="true"
      />
      <button
        type="button"
        class="chip"
        :disabled="testRunning"
        v-tip="'Test download and upload speed\nAbout 15 seconds, up to 250 MB'"
        @mousedown.prevent
        @click="runSpeedTest"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.gauge" />
        Speed test
      </button>
    </div>

    <div class="net-head">
      <span class="net-pill" :class="tone">{{ pill }}</span>
      <span class="net-verdict">{{ verdict }}</span>
    </div>

    <div v-if="chips.length" class="net-addr">
      <button
        v-for="chip in chipsShown"
        :key="chip.value"
        type="button"
        class="net-chip"
        v-tip="'Copy'"
        @mousedown.prevent
        @click="copy(chip.value, chip.value)"
      >
        {{ chip.label }} <b>{{ chip.value }}</b>
      </button>
      <span v-if="chips.length > chipsShown.length" class="net-chip more"
        >+{{ chips.length - chipsShown.length }} more</span
      >
    </div>

    <p v-if="notice" class="notice">{{ notice }}</p>

    <ActionMenu
      v-if="actionsOpen"
      id="net-actions"
      class="res-actions"
      :actions="actions"
      :index="actionIndex"
      @run="menu.run"
      @hover="menu.hover"
      @fit="menu.shown.value = $event"
    />

    <template v-else>
      <div class="net-chart" @mousemove="hoverChart" @mouseleave="hovered = -1">
        <svg
          ref="svgEl"
          :viewBox="`0 0 ${W} ${H}`"
          role="img"
          aria-label="Latency, loss and throughput over the last 15 minutes"
        >
          <defs>
            <linearGradient id="net-fill" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" class="net-stop" stop-opacity="0.32" />
              <stop offset="1" class="net-stop" stop-opacity="0" />
            </linearGradient>
            <linearGradient id="net-down-fill" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" class="net-stop" stop-opacity="0.05" />
              <stop offset="1" class="net-stop" stop-opacity="0.5" />
            </linearGradient>
            <linearGradient id="net-up-fill" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" class="net-stop-up" stop-opacity="0.5" />
              <stop offset="1" class="net-stop-up" stop-opacity="0.05" />
            </linearGradient>
          </defs>
          <rect
            v-for="gap in chart.gaps"
            :key="`gap-${gap.x}`"
            class="net-gap"
            :x="gap.x"
            :y="LT"
            :width="bw + 0.5"
            :height="LH"
          />
          <g v-for="line in grid" :key="line.value">
            <line class="net-grid" :x1="PL" :x2="W - PR" :y1="line.y" :y2="line.y" />
            <text class="net-axis" :x="W - PR + 6" :y="line.y + 3.5">
              {{ line.value }}{{ line.value === MAX_MS ? '+' : '' }} ms
            </text>
          </g>
          <template v-for="tick in chart.ticks" :key="`loss-${tick.x}`">
            <rect
              v-if="tick.drop"
              class="net-drop"
              :x="tick.x"
              :y="LT"
              :width="bw + 0.5"
              :height="LH"
            />
            <rect
              class="net-loss"
              :x="tick.x"
              :y="LT - 8"
              :width="Math.max(1.5, bw - 0.5)"
              height="4"
              rx="1"
            />
          </template>
          <line
            v-for="mark in chart.marks"
            :key="`mark-${mark.x}`"
            class="net-change"
            :x1="mark.x"
            :x2="mark.x"
            :y1="LT"
            :y2="TT + TH"
          />
          <line
            v-for="at in chart.tests"
            :key="`test-${at}`"
            class="net-test"
            :x1="at"
            :x2="at"
            :y1="LT"
            :y2="TT + TH"
          />
          <path class="net-area" :d="chart.area" />
          <path class="net-band" :d="chart.band" />
          <path class="net-line" :d="chart.line" />
          <template v-if="chart.now">
            <circle class="net-now-halo" :cx="chart.now.x" :cy="chart.now.y" r="6" />
            <circle class="net-now" :cx="chart.now.x" :cy="chart.now.y" r="3" />
          </template>
          <text class="net-axis" :x="PL" :y="TT - 4">{{ used }}</text>
          <path class="net-down" :d="strip.down" />
          <path class="net-up" :d="strip.up" />
          <line class="net-grid" :x1="PL" :x2="W - PR" :y1="strip.mid" :y2="strip.mid" />
          <text class="net-axis" :x="W - PR + 6" :y="TT + 8">{{ bytes(strip.top) }}/s</text>
          <text class="net-axis faint" :x="PL" :y="H - 1">−15m</text>
          <text class="net-axis faint" :x="PL + IW / 3" :y="H - 1" text-anchor="middle">−10m</text>
          <text class="net-axis faint" :x="PL + (IW * 2) / 3" :y="H - 1" text-anchor="middle">
            −5m
          </text>
          <text class="net-axis faint" :x="W - PR" :y="H - 1" text-anchor="end">now</text>
          <line v-if="tip" class="net-cursor" :x1="tip.x" :x2="tip.x" :y1="LT" :y2="TT + TH" />
        </svg>
        <div v-if="tip" class="net-tip" :style="{ left: tip.left }">{{ tip.text }}</div>
      </div>

      <SpeedPanel v-if="testing || result" :testing="testing" :result="result" />
      <div v-else class="net-hops" role="table" aria-label="Hops">
        <div class="h" role="columnheader">Hop</div>
        <div class="h num" role="columnheader">Now</div>
        <div class="h num" role="columnheader">Median</div>
        <div class="h num" role="columnheader">p95</div>
        <div class="h num" role="columnheader">Jitter</div>
        <div class="h num" role="columnheader">Loss</div>
        <template v-for="hop in hops" :key="hop.name">
          <div class="name" role="cell">
            {{ hop.name }}
            <a
              v-if="hop.link"
              :href="hop.link"
              class="net-router-link"
              v-tip="'Open the router\'s sign-in page'"
              @mousedown.prevent
              @click.prevent="openRouter(hop.link)"
              >{{ hop.where }}</a
            >
            <small v-else>{{ hop.where }}</small>
          </div>
          <div class="num now" :class="hop.nowTone" role="cell">{{ hop.now }}</div>
          <div class="num" :class="hop.medianTone" role="cell">{{ hop.median }}</div>
          <div class="num" :class="hop.slowTone" role="cell">{{ hop.p95 }}</div>
          <div class="num" :class="hop.jitterTone" role="cell">{{ hop.jitter }}</div>
          <div class="num" :class="hop.lossTone" role="cell">{{ hop.loss }}</div>
        </template>
      </div>
    </template>

    <footer class="footer">
      <KeyHints :hints="actionsOpen ? MENU_HINTS : testRunning ? TEST_HINTS : LIST_HINTS" />
      <span v-if="props.lastSpeed && !testing && !result">{{ lastTestLine(props.lastSpeed) }}</span>
    </footer>
  </div>
</template>
