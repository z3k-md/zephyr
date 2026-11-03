// Countdown timers ("timer 5m tea") and focus sessions ("focus 50m", "focus deep work"):
// the row that starts one, the presets under a bare command, and running ones ticking down.
// Enter on a running timer cancels it; Enter on a running focus session ends it.

import { onUnmounted, ref, watch, type Ref } from 'vue';
import { errorMessage, timerCancel, timerStart } from '../api';
import type { Countdown, Snapshot, SuggestionRow } from '../types';
import { barShown } from '../visibility';
import {
  DEFAULT_FOCUS_SECONDS,
  FOCUS_PRESETS,
  MAX_TIMER_SECONDS,
  parseFocus,
  parseTimer,
  TIMER_PRESETS,
  timeLeft,
  timerLength,
  type FocusAsk,
} from './timers';

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

  function runningRow(timer: Countdown): SuggestionRow {
    const left = timeLeft(timer.at - now.value / 1000);
    const name = timer.focus
      ? timer.label || `${timerLength(timer.seconds)} focus`
      : timer.label || `${timerLength(timer.seconds)} timer`;
    return {
      kind: 'timerPending',
      label: `${name} · ${left}`,
      query: '',
      target: '',
      hint: timer.focus ? 'Enter ends' : 'Enter cancels',
      keyHints: [{ keys: 'enter', label: timer.focus ? 'ends' : 'cancels' }],
      section: timer.focus ? 'focus' : 'timer',
      icon: timer.focus ? 'glyph:focus' : 'glyph:clock',
      timer: { id: timer.id, seconds: timer.seconds, label: timer.label, focus: timer.focus },
    };
  }

  function startRow(seconds: number, label: string, focus: boolean): SuggestionRow {
    const noun = focus ? 'focus' : 'timer';
    return {
      kind: 'timer',
      label: `Start a ${timerLength(seconds)} ${noun}${label ? ` · ${label}` : ''}`,
      query: '',
      target: '',
      hint: 'Start',
      section: focus ? 'focus' : 'timer',
      icon: focus ? 'glyph:focus' : 'glyph:clock',
      timer: { seconds, label, focus },
    };
  }

  /** A bare "focus" offers the presets; a name with no length uses Settings. */
  function focusLengths(ask: FocusAsk): number[] {
    if (ask.seconds !== null) return [ask.seconds];
    if (!ask.label) return FOCUS_PRESETS;
    const saved = snapshot.value?.focus?.seconds;
    return [saved && saved >= 1 && saved <= MAX_TIMER_SECONDS ? saved : DEFAULT_FOCUS_SECONDS];
  }

  /** Timer and focus rows to lead the list with, and which one takes Enter (null leaves it). */
  function timerRows(): { rows: SuggestionRow[]; preselect: number | null } {
    const text = typed().trim();
    const ask = parseTimer(typed());
    const focusAsk = parseFocus(typed());
    const timers = [...(snapshot.value?.timers ?? [])].sort((a, b) => a.at - b.at);
    const rows: SuggestionRow[] = [];

    // A running focus session sits above the presets of a bare "focus".
    if (!text || focusAsk) {
      for (const timer of timers) {
        if (timer.focus) rows.push(runningRow(timer));
      }
    }
    let preselect: number | null = null;
    if (focusAsk) {
      const bare = focusAsk.seconds === null && focusAsk.label === '';
      const start = focusLengths(focusAsk).map((seconds) =>
        startRow(seconds, focusAsk.label, true)
      );
      if (bare) {
        preselect = rows.length;
        rows.push(...start);
      } else {
        rows.unshift(...start);
        preselect = 0;
      }
    }

    const timerStart = rows.length;
    if (!text || ask) {
      for (const timer of timers) {
        if (!timer.focus) rows.push(runningRow(timer));
      }
    }
    if (ask) {
      const start = (ask.seconds === null ? TIMER_PRESETS : [ask.seconds]).map((seconds) =>
        startRow(seconds, ask.label, false)
      );
      const at = rows.length;
      rows.push(...start);
      // The start row takes Enter, ahead of running timers.
      if (at > timerStart) rows.splice(timerStart, 0, ...rows.splice(at));
      preselect = timerStart;
    }

    return { rows, preselect };
  }

  async function startTimer(seconds: number, label: string, focus = false) {
    const replaced = focus ? snapshot.value?.timers?.find((timer) => timer.focus) : undefined;
    try {
      await timerStart(seconds, label, focus);
      query.value = '';
      notice.value = replaced ? `Replaced ${replaced.label || 'the focus session'}` : null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function cancelTimer(id: number, focus = false) {
    try {
      await timerCancel(id);
      notice.value = focus ? 'Focus ended' : 'Timer cancelled';
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  return { timerRows, startTimer, cancelTimer, now };
}
