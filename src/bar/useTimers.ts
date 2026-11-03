// Countdown timers ("timer 5m tea"): the row that starts one, the presets under a bare
// "timer", and running timers ticking down, each cancelled with Enter.

import { onUnmounted, ref, watch, type Ref } from 'vue';
import { errorMessage, timerCancel, timerStart } from '../api';
import type { Snapshot, SuggestionRow } from '../types';
import { barShown } from '../visibility';
import { parseTimer, TIMER_PRESETS, timeLeft, timerLength } from './timers';

export function useTimers(ctx: {
  snapshot: Ref<Snapshot | null>;
  /** The text the shown results are for. */
  typed: () => string;
  query: Ref<string>;
  notice: Ref<string | null>;
}) {
  const { snapshot, typed, query, notice } = ctx;

  // Running timers count down by the second while the bar shows them.
  const now = ref(Date.now());
  let tick = 0;
  watch(
    () => barShown.value && !!snapshot.value?.timers?.length,
    (ticking) => {
      window.clearInterval(tick);
      tick = 0;
      if (!ticking) return;
      now.value = Date.now();
      tick = window.setInterval(() => (now.value = Date.now()), 1000);
    },
    { immediate: true }
  );
  onUnmounted(() => window.clearInterval(tick));

  /** Timer rows to lead the list with, and whether the first takes Enter. */
  function timerRows(): { rows: SuggestionRow[]; preselect: boolean } {
    const ask = parseTimer(typed());
    const text = typed().trim();
    const rows: SuggestionRow[] = [];
    // Running timers show on the empty bar and under "timer".
    if (ask || !text) {
      const running = [...(snapshot.value?.timers ?? [])].sort((a, b) => a.at - b.at);
      for (const timer of running) {
        const left = timeLeft(timer.at - now.value / 1000);
        rows.push({
          kind: 'timerPending',
          label: `${timer.label || timerLength(timer.seconds) + ' timer'} · ${left}`,
          query: '',
          target: '',
          hint: 'Enter cancels',
          keyHints: [{ keys: 'enter', label: 'cancels' }],
          section: 'timer',
          icon: 'glyph:clock',
          timer: { id: timer.id, seconds: timer.seconds, label: timer.label },
        });
      }
    }
    if (!ask) return { rows, preselect: false };
    const lengths = ask.seconds === null ? TIMER_PRESETS : [ask.seconds];
    const first = rows.length;
    for (const seconds of lengths) {
      rows.push({
        kind: 'timer',
        label: `Start a ${timerLength(seconds)} timer${ask.label ? ` · ${ask.label}` : ''}`,
        query: '',
        target: '',
        hint: 'Start',
        section: 'timer',
        icon: 'glyph:clock',
        timer: { seconds, label: ask.label },
      });
    }
    // The start row takes Enter, ahead of running timers.
    if (first) rows.unshift(...rows.splice(first));
    return { rows, preselect: true };
  }

  async function startTimer(seconds: number, label: string) {
    try {
      await timerStart(seconds, label);
      query.value = '';
      notice.value = null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function cancelTimer(id: number) {
    try {
      await timerCancel(id);
      notice.value = 'Timer cancelled';
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  return { timerRows, startTimer, cancelTimer };
}
