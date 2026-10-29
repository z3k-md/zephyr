// Timers for components: a refresh loop that runs only while someone can see it, and
// one-off delays that die with the component.

import { computed, onScopeDispose, toValue, watch, type MaybeRefOrGetter } from 'vue';
import { barShown, pageShown } from './visibility';

/**
 * Runs `task` now, then again `ms` after each run finishes, while `active` holds and the
 * window can be seen (the bar on screen, the page not hidden). There's only ever one loop:
 * a run still out when the loop restarts doesn't schedule another.
 */
export function usePoll(
  task: () => unknown,
  ms: MaybeRefOrGetter<number>,
  active: MaybeRefOrGetter<boolean> = true
) {
  const running = computed(() => toValue(active) && barShown.value && pageShown.value);
  let loop = 0;
  let timer = 0;

  async function run(id: number) {
    try {
      await task();
    } catch {
      // The task says what went wrong itself; the loop carries on.
    }
    if (id === loop) timer = window.setTimeout(() => void run(id), toValue(ms));
  }

  function stop() {
    loop += 1;
    window.clearTimeout(timer);
    timer = 0;
  }

  /** Runs it now and starts the wait over, if it should be running. */
  function restart() {
    stop();
    if (running.value) void run(loop);
  }

  watch(running, restart, { immediate: true, flush: 'sync' });
  onScopeDispose(stop);
  return { restart };
}

/** `setTimeout` for a component: whatever is still waiting when it unmounts is dropped. */
export function useLater() {
  const timers = new Set<number>();
  onScopeDispose(() => {
    for (const timer of timers) window.clearTimeout(timer);
    timers.clear();
  });
  return (callback: () => unknown, ms: number) => {
    const timer = window.setTimeout(() => {
      timers.delete(timer);
      void callback();
    }, ms);
    timers.add(timer);
  };
}
