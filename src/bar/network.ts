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

/** "412↓ / 38↑ Mb/s, lag under load B (good)". */
export function speedLine(result: SpeedResult): string {
  const lag = BLOAT_WORDS[result.bloat]
    ? `lag under load ${result.bloat} (${bloatWord(result.bloat)})`
    : 'lag under load not measured';
  return `${mbps(result.downMbps)}↓ / ${mbps(result.upMbps)}↑ Mb/s, ${lag}`;
}

/**
 * The bufferbloat grade in words. The test grades how much the ping grows while the line is
 * busy downloading or uploading (see bloat_grade in net/speed.rs).
 */
const BLOAT_WORDS: Record<string, string> = {
  A: 'excellent',
  B: 'good',
  C: 'noticeable',
  D: 'poor',
};

export function bloatWord(grade: string): string {
  return BLOAT_WORDS[grade] ?? 'not measured';
}

export const BLOAT_TIP = [
  'How much slower the connection reacts while it is busy (known as bufferbloat).',
  'When it is high, calls and games stutter while something downloads.',
  '',
  'A  under 30 ms extra · B  under 60 ms',
  'C  under 200 ms · D  200 ms or more',
  '',
  'C or D: turn on Smart Queue (SQM, fq_codel or CAKE) on your router.',
].join('\n');

export function mbps(value: number): string {
  return value < 10 ? value.toFixed(1) : String(Math.round(value));
}

/** "Last test 12 min ago: 412↓ / 38↑ Mb/s, lag under load B (good)". */
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
