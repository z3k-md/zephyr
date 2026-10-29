// The connection monitor as the bar shows it: numbers in words, the sparkline, and a
// summary that refreshes once a second while something on screen shows it.

import { ref, type Ref } from 'vue';
import { networkSummary } from '../api';
import { usePoll } from '../poll';
import { ago } from '../format';
import type { NetworkGrade, NetworkSummary, SpeedResult } from '../types';

export const GRADE_NAMES: Record<NetworkGrade, string> = {
  good: 'Good',
  fair: 'Fair',
  poor: 'Poor',
};

/** "18", "4.2", "—". */
export function ms(value: number | null | undefined): string {
  if (value === null || value === undefined) return '—';
  if (value < 10) return value.toFixed(1).replace(/\.0$/, '');
  return String(Math.round(value));
}

/** Loss as a percentage: "0", "0.3", "12". */
export function lossText(loss: number): string {
  const percent = loss * 100;
  if (percent === 0) return '0';
  return percent < 10 ? percent.toFixed(1) : String(Math.round(percent));
}

/** "18 ms · jitter 2 ms · 0% loss". */
export function headline(summary: NetworkSummary): string {
  if (summary.offline) return 'Offline';
  if (summary.median === null) return 'Measuring…';
  return `${ms(summary.median)} ms · jitter ${ms(summary.jitter)} ms · ${lossText(summary.loss)}% loss`;
}

/** The verdict without its first sentence, which the headline already says. */
export function note(summary: NetworkSummary): string {
  if (!summary.watching) return 'Not watching: turn it on in Settings › Network';
  if (summary.median === null) return summary.verdict;
  const rest = summary.verdict.replace(/^.*?% loss\.\s*/, '');
  return rest || (summary.grade === 'good' ? 'Steady, no drops' : summary.verdict);
}

/** "412↓ / 38↑ Mb/s, bloat B". */
export function speedLine(result: SpeedResult): string {
  return `${mbps(result.downMbps)}↓ / ${mbps(result.upMbps)}↑ Mb/s, bloat ${result.bloat}`;
}

export function mbps(value: number): string {
  return value < 10 ? value.toFixed(1) : String(Math.round(value));
}

/** "Last test 12 min ago: 412↓ / 38↑ Mb/s, bloat B". */
export function lastTestLine(result: SpeedResult | null | undefined): string {
  if (!result) return '';
  return `Last test ${ago(result.at / 1000)}: ${speedLine(result)}`;
}

/**
 * A median line across `width`×`height`, 0 to 100+ ms bottom to top, broken where nothing
 * came back; marks are the stretches with no reply at all.
 */
export function sparkPath(values: (number | null)[], width: number, height: number) {
  const step = width / Math.max(1, values.length - 1);
  const y = (value: number) => height - 3 - (Math.min(value, 100) / 100) * (height - 8);
  let line = '';
  let pen = false;
  values.forEach((value, index) => {
    if (value === null) {
      pen = false;
      return;
    }
    line += `${pen ? 'L' : 'M'}${(index * step).toFixed(1)} ${y(value).toFixed(1)}`;
    pen = true;
  });
  return line;
}

/** The summary, fetched now and every second while `active` holds. */
export function useNetworkSummary(active: Ref<boolean>) {
  const summary = ref<NetworkSummary | null>(null);
  async function load() {
    try {
      summary.value = await networkSummary();
    } catch {
      summary.value = null;
    }
  }
  // Paused with the bar: hidden, nothing shows it.
  usePoll(load, 1000, active);
  return summary;
}
