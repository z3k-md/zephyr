// The ticker card: a stock or crypto typed on its own ("TSLA", "eth", "$aapl"). The quote is
// looked up a moment after typing stops, and refreshed every half minute while it shows.
// Tab and Shift+Tab step through the chart's spans; a new symbol starts on the day again.

import { computed, onUnmounted, ref, watch, type Ref } from 'vue';
import { errorMessage, openUrl, quote } from '../api';
import type { Quote, QuoteRange } from '../types';
import { tickerFor, type Ticker } from './ticker';
import { barShown } from '../visibility';

const REFRESH_MS = 30_000;
export const RANGES: { range: QuoteRange; label: string }[] = [
  { range: '1d', label: '1D' },
  { range: '5d', label: '5D' },
  { range: '1mo', label: '1M' },
  { range: '1y', label: '1Y' },
];

export function useTickerCard(ctx: {
  query: Ref<string>;
  /** The card shows on the main bar with no scope, and gives way to the ⌘K menu. */
  shown: () => boolean;
  menuOpen: Ref<boolean>;
  notice: Ref<string | null>;
  clearAfterOpen: () => void;
}) {
  const { query, menuOpen, notice } = ctx;
  // Hidden, there's no card, so the refresh stops; showing again refreshes at once.
  const ticker = computed(() => (ctx.shown() && barShown.value ? tickerFor(query.value) : null));
  const symbolKey = (asked: Ticker | null) => (asked ? `${asked.symbol}:${asked.crypto}` : '');
  const range = ref<QuoteRange>('1d');
  const key = computed(() => (ticker.value ? `${symbolKey(ticker.value)}:${range.value}` : ''));
  const found = ref<{ key: string; symbol: string; quote: Quote | null } | null>(null);

  let timer = 0;
  function lookUp(asked: Ticker, span: QuoteRange, delay: number) {
    window.clearTimeout(timer);
    const wanted = `${symbolKey(asked)}:${span}`;
    timer = window.setTimeout(async () => {
      const result = await quote(asked.symbol, asked.crypto, span).catch(() => undefined);
      if (key.value !== wanted) return;
      // A failed refresh keeps the last quote rather than blanking the card.
      if (result !== undefined || found.value?.key !== wanted) {
        found.value = { key: wanted, symbol: symbolKey(asked), quote: result ?? null };
      }
      lookUp(asked, span, REFRESH_MS);
    }, delay);
  }
  watch(
    () => symbolKey(ticker.value),
    (now, before) => {
      if (now && before && now !== before) range.value = '1d';
    }
  );
  watch(key, (now) => {
    window.clearTimeout(timer);
    if (!ticker.value) return;
    // Typing waits a moment; picking a span, or coming back to the same one, asks at once.
    const quick = found.value?.symbol === symbolKey(ticker.value);
    lookUp(ticker.value, range.value, quick || found.value?.key === now ? 0 : 300);
  });
  onUnmounted(() => window.clearTimeout(timer));

  // While another span loads, the last one stays up rather than the card blinking out.
  const cardQuote = computed(() => {
    if (!ticker.value || menuOpen.value) return null;
    return found.value?.symbol === symbolKey(ticker.value) ? found.value.quote : null;
  });

  /** Tab on the card: the next span (Shift+Tab, the one before). */
  function stepRange(by: 1 | -1) {
    const at = RANGES.findIndex((option) => option.range === range.value);
    range.value = RANGES[(at + by + RANGES.length) % RANGES.length].range;
  }

  function pickRange(picked: QuoteRange) {
    range.value = picked;
  }

  /** Ctrl+Enter on the card: the full quote on Yahoo Finance. */
  function openQuote() {
    const shown = cardQuote.value;
    if (!shown) return;
    void openUrl(`https://finance.yahoo.com/quote/${encodeURIComponent(shown.symbol)}`)
      .then(ctx.clearAfterOpen)
      .catch((error) => (notice.value = errorMessage(error)));
  }

  return { cardQuote, quoteRange: range, stepRange, pickRange, openQuote };
}
