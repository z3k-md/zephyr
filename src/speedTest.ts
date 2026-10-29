import { onMounted, onUnmounted, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { errorMessage, speedTest, speedTestCancel } from './api';
import type { SpeedProgress, SpeedResult } from './types';

/**
 * A speed test run from this window: its live reading while it runs (the "speed-test" events)
 * and its result once done. Shared by the Network view and Settings.
 */
export function useSpeedTest() {
  const running = ref(false);
  const testing = ref<SpeedProgress | null>(null);
  const result = ref<SpeedResult | null>(null);

  let unlisten: UnlistenFn | null = null;
  let unmounted = false;
  onMounted(async () => {
    const off = await listen<SpeedProgress>('speed-test', (event) => {
      if (running.value) testing.value = event.payload;
    });
    if (unmounted) off();
    else unlisten = off;
  });
  onUnmounted(() => {
    unmounted = true;
    unlisten?.();
  });

  /** Runs a test. Resolves to its error, or '' when it finished or was cancelled. */
  async function run(): Promise<string> {
    if (running.value) return '';
    result.value = null;
    running.value = true;
    testing.value = { phase: 'ping', mbps: null, ping: null, done: 0 };
    try {
      result.value = await speedTest();
      return '';
    } catch (error) {
      const message = errorMessage(error);
      return message === 'Cancelled' ? '' : message;
    } finally {
      running.value = false;
      testing.value = null;
    }
  }

  function cancel() {
    void speedTestCancel().catch(() => undefined);
  }

  return { running, testing, result, run, cancel };
}
