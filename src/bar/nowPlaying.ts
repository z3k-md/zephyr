import { computed, onMounted, onUnmounted, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { nowPlaying, nowPlayingControl, nowPlayingFocus } from '../api';
import type { MediaControl, NowPlaying } from '../types';

/** How long a paused song stays in the footer. */
const LINGER_MS = 2 * 60 * 1000;

/**
 * The song or video playing on this computer, kept current from the `nowplaying` events, and
 * the buttons for it. `shown` is what the footer draws: something playing, or paused within
 * the last couple of minutes.
 */
export function useNowPlaying() {
  const track = ref<NowPlaying | null>(null);
  const now = ref(Date.now());
  let unlisten: UnlistenFn | null = null;
  let clock = 0;
  let recheck = 0;

  onMounted(async () => {
    track.value = await nowPlaying().catch(() => null);
    unlisten = await listen<NowPlaying | null>('nowplaying', (event) => {
      now.value = Date.now();
      const before = track.value;
      const next = event.payload;
      // A song that was playing a moment ago counts from now, so pausing never hides it.
      if (next && !next.playing && before?.playing && before.sourceApp === next.sourceApp) {
        next.lastPlayedAt = Math.max(next.lastPlayedAt ?? 0, now.value);
      }
      track.value = next;
    });
    clock = window.setInterval(() => (now.value = Date.now()), 15000);
  });

  onUnmounted(() => {
    unlisten?.();
    window.clearInterval(clock);
    window.clearTimeout(recheck);
  });

  const shown = computed(() => {
    const current = track.value;
    if (!current) return null;
    if (current.playing) return current;
    const played = current.lastPlayedAt;
    return played && now.value - played < LINGER_MS ? current : null;
  });

  function press(control: MediaControl) {
    const current = track.value;
    if (!current) return;
    // The button answers at once; the app's own report follows and corrects it if the press
    // didn't take.
    if (control === 'toggle') {
      track.value = { ...current, playing: !current.playing, lastPlayedAt: Date.now() };
    }
    void nowPlayingControl(control).catch(() => undefined);
    window.clearTimeout(recheck);
    recheck = window.setTimeout(async () => {
      const fresh = await nowPlaying().catch(() => undefined);
      if (fresh !== undefined) track.value = fresh;
    }, 1500);
  }

  async function focus() {
    await nowPlayingFocus().catch(() => false);
  }

  return { shown, press, focus, tick: () => (now.value = Date.now()) };
}
