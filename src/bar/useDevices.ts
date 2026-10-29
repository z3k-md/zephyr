// Other computers asking to join sync: a card on the bar, answered with ⌘Y or ⌘N (Ctrl
// elsewhere), never plain Enter.

import { onUnmounted, ref, type Ref } from 'vue';
import { errorMessage, syncApprove, syncPending, syncRevoke } from '../api';
import type { SyncPending } from '../types';

// Asked on a summon at most every five minutes while none are waiting, and every minute
// while one is (so its card goes once it's answered elsewhere).
const DEVICE_CHECK_MS = 5 * 60 * 1000;
const DEVICE_POLL_MS = 60000;

export function useDevices(notice: Ref<string | null>) {
  const devicesWaiting = ref<SyncPending[]>([]);
  let devicePoll = 0;
  let devicesCheckedAt = 0;
  // The minute's check stops while the bar is hidden.
  let running = true;

  async function loadDevices(force = false) {
    const recent = Date.now() - devicesCheckedAt < DEVICE_CHECK_MS;
    if (!force && recent && devicesWaiting.value.length === 0) return;
    devicesCheckedAt = Date.now();
    try {
      devicesWaiting.value = await syncPending();
    } catch {
      devicesWaiting.value = [];
    }
    pollDevices();
  }

  /** The minute's check runs only while a computer is waiting and the bar shows. */
  function pollDevices() {
    const wanted = running && devicesWaiting.value.length > 0;
    if (wanted && !devicePoll) {
      devicePoll = window.setInterval(() => void loadDevices(true), DEVICE_POLL_MS);
    } else if (!wanted && devicePoll) {
      window.clearInterval(devicePoll);
      devicePoll = 0;
    }
  }

  function pause() {
    running = false;
    pollDevices();
  }

  function resume() {
    running = true;
    pollDevices();
  }

  /** An answer still on its way: the next request moves up only once it's done, and a
   * second key press mustn't answer that one unseen. */
  let answering = false;

  async function answerDevice(decision: 'allow' | 'deny') {
    const device = devicesWaiting.value[0];
    if (!device || answering) return;
    answering = true;
    try {
      if (decision === 'allow') await syncApprove(device.id);
      else await syncRevoke(device.id);
      await loadDevices(true);
    } catch (error) {
      notice.value = errorMessage(error);
    } finally {
      answering = false;
    }
  }

  /** ⌘Y or ⌘N while a computer is waiting; whether it took the key. */
  function onKey(event: KeyboardEvent): boolean {
    if (!devicesWaiting.value.length) return false;
    if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return false;
    if (event.code !== 'KeyY' && event.code !== 'KeyN') return false;
    event.preventDefault();
    // A held key answers once; the next request needs its own press.
    if (!event.repeat) void answerDevice(event.code === 'KeyY' ? 'allow' : 'deny');
    return true;
  }

  onUnmounted(() => window.clearInterval(devicePoll));

  return { devicesWaiting, loadDevices, answerDevice, pause, resume, onKey };
}
