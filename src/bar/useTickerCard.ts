// The ticker card: a stock or crypto typed on its own ("TSLA", "eth", "$aapl"). The quote is
// looked up a moment after typing stops, and refreshed every half minute while it shows.

import { computed, onUnmounted, ref, watch, type Ref } from 'vue';
import { errorMessage, openUrl, quote } from '../api';
import type { Quote } from '../types';
import { tickerFor, type Ticker } from './ticker';
import { barShown } from '../visibility';

const REFRESH_MS = 30_000;

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
  const key = (asked: Ticker | null) => (asked ? `${asked.symbol}:${asked.crypto}` : '');
  const found = ref<{ key: string; quote: Quote | null } | null>(null);

  let timer = 0;
  function lookUp(asked: Ticker, delay: number) {
    window.clearTimeout(timer);
    timer = window.setTimeout(async () => {
      const result = await quote(asked.symbol, asked.crypto).catch(() => undefined);
      if (key(ticker.value) !== key(asked)) return;
      // A failed refresh keeps the last quote rather than blanking the card.
      if (result !== undefined || found.value?.key !== key(asked)) {
        found.value = { key: key(asked), quote: result ?? null };
      }
      lookUp(asked, REFRESH_MS);
    }, delay);
  }
  watch(
    () => key(ticker.value),
    () => {
      window.clearTimeout(timer);
      if (ticker.value) lookUp(ticker.value, found.value?.key === key(ticker.value) ? 0 : 300);
    }
  );
  onUnmounted(() => window.clearTimeout(timer));

  const cardQuote = computed(() => {
    if (!ticker.value || menuOpen.value) return null;
    return found.value?.key === key(ticker.value) ? found.value.quote : null;
  });

  /** Ctrl+Enter on the card: the full quote on Yahoo Finance. */
  function openQuote() {
    const shown = cardQuote.value;
    if (!shown) return;
    void openUrl(`https://finance.yahoo.com/quote/${encodeURIComponent(shown.symbol)}`)
      .then(ctx.clearAfterOpen)
      .catch((error) => (notice.value = errorMessage(error)));
  }

  return { cardQuote, openQuote };
}
