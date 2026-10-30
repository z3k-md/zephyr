<script setup lang="ts">
  import { computed } from 'vue';
  import type { Quote } from '../types';
  import KeyHints from '../components/KeyHints.vue';

  // The ticker card over the results for a stock or crypto ("TSLA", "eth"): the price, its
  // change since the last close (crypto: the last 24 hours) and a chart of that stretch, with
  // the reference price as a dashed line. Enter stays on the search row; Ctrl+Enter opens the
  // full quote.
  const props = defineProps<{ quote: Quote }>();
  defineEmits<{ open: [] }>();

  const change = computed(() => props.quote.price - props.quote.reference);
  const percent = computed(() =>
    props.quote.reference ? (change.value / props.quote.reference) * 100 : 0
  );
  const tone = computed(() => (change.value > 0 ? 'up' : change.value < 0 ? 'down' : ''));

  /** Cents for most prices; more digits for coins worth less than a dollar. */
  function money(value: number, signed = false) {
    const digits = Math.abs(value) < 1 && value !== 0 ? 4 : 2;
    try {
      return new Intl.NumberFormat(undefined, {
        style: 'currency',
        currency: props.quote.currency || 'USD',
        minimumFractionDigits: digits,
        maximumFractionDigits: digits,
        signDisplay: signed ? 'exceptZero' : 'auto',
      }).format(value);
    } catch {
      return value.toFixed(digits);
    }
  }

  const changeText = computed(
    () =>
      `${money(change.value, true)} (${percent.value > 0 ? '+' : ''}${percent.value.toFixed(2)}%)`
  );
  const span = computed(() => (props.quote.crypto ? 'Past 24 hours' : 'Today'));
  const tradedAt = computed(() =>
    props.quote.tradedAt
      ? new Date(props.quote.tradedAt).toLocaleTimeString(undefined, {
          hour: 'numeric',
          minute: '2-digit',
        })
      : ''
  );

  const range = computed(() => {
    const prices = props.quote.points.map(([, price]) => price);
    if (!prices.length) return null;
    return { low: Math.min(...prices), high: Math.max(...prices) };
  });

  // The chart on a 100 × 40 box: the line, the area under it, and the reference line.
  const chart = computed(() => {
    const points = props.quote.points;
    if (points.length < 2) return null;
    const prices = points.map(([, price]) => price);
    const low = Math.min(...prices, props.quote.reference);
    const high = Math.max(...prices, props.quote.reference);
    const height = high - low || 1;
    const first = points[0][0];
    const width = points[points.length - 1][0] - first || 1;
    const y = (price: number) => 38 - ((price - low) / height) * 36;
    const line = points
      .map(
        ([time, price]) => `${(((time - first) / width) * 100).toFixed(2)},${y(price).toFixed(2)}`
      )
      .join(' ');
    return { line, area: `0,40 ${line} 100,40`, reference: y(props.quote.reference) };
  });
</script>

<template>
  <section class="ticker-card" :class="tone" aria-label="Quote" @mousedown.prevent>
    <div class="ticker-head">
      <span class="ticker-name">
        {{ quote.name }} <span class="ticker-symbol">{{ quote.symbol }}</span>
      </span>
      <KeyHints :hints="[{ keys: 'mod+enter', label: 'Yahoo Finance' }]" @click="$emit('open')" />
    </div>
    <div class="ticker-main">
      <span class="ticker-price">{{ money(quote.price) }}</span>
      <span class="ticker-detail">
        <b class="ticker-change">{{ changeText }}</b>
        <span
          >{{ span }}<template v-if="tradedAt"> · as of {{ tradedAt }}</template></span
        >
      </span>
      <span v-if="range" class="ticker-range">
        <span>High {{ money(range.high) }}</span>
        <span>Low {{ money(range.low) }}</span>
      </span>
    </div>
    <svg
      v-if="chart"
      class="ticker-chart"
      viewBox="0 0 100 40"
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <polygon :points="chart.area" />
      <line x1="0" x2="100" :y1="chart.reference" :y2="chart.reference" />
      <polyline :points="chart.line" />
    </svg>
  </section>
</template>
