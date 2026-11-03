import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { nowPlaying, nowPlayingControl, nowPlayingFocus } from '../api';
import type { MediaControl, NowPlaying } from '../types';

/** How long a paused song stays in the bar. */
const LINGER_MS = 2 * 60 * 1000;

/**
 * The song or video playing on this computer, kept current from the `nowplaying` events, and
 * the buttons for it. `shown` is what the bar draws: something playing, or paused within
 * the last couple of minutes.
 */
export function useNowPlaying() {
  const track = ref<NowPlaying | null>(null);
  const now = ref(Date.now());
  let unlisten: UnlistenFn | null = null;
  let clock = 0;
  let recheck = 0;
  /** The second hand for the position, running only while a song plays in the shown bar. */
  let ticker = 0;
  let awake = false;

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
    resume();
  });

  // The clock only matters while the bar shows; the bar pauses it while hidden.
  function pause() {
    awake = false;
    window.clearInterval(clock);
    clock = 0;
    tick();
  }

  function resume() {
    awake = true;
    now.value = Date.now();
    if (!clock) clock = window.setInterval(() => (now.value = Date.now()), 15000);
    tick();
  }

  function tick() {
    const run = awake && !!track.value?.playing && track.value.positionMs !== null;
    if (run && !ticker) ticker = window.setInterval(() => (now.value = Date.now()), 1000);
    if (!run && ticker) {
      window.clearInterval(ticker);
      ticker = 0;
    }
  }

  watch(() => [track.value?.playing, track.value?.positionMs], tick);

  onUnmounted(() => {
    unlisten?.();
    window.clearInterval(clock);
    window.clearInterval(ticker);
    window.clearTimeout(recheck);
  });

  const shown = computed(() => {
    const current = track.value;
    if (!current) return null;
    if (current.playing) return current;
    const played = current.lastPlayedAt;
    return played && now.value - played < LINGER_MS ? current : null;
  });

  /** Where the shown song is now: its last report, counted on while it plays. */
  const positionMs = computed(() => {
    const current = shown.value;
    if (current?.positionMs == null) return null;
    const moved = current.playing ? Math.max(0, now.value - current.updatedAt) : 0;
    const at = current.positionMs + moved;
    return current.durationMs ? Math.min(at, current.durationMs) : at;
  });

  function press(control: MediaControl) {
    const current = track.value;
    if (!current) return;
    // The button answers at once; the app's own report follows and corrects it if the press
    // didn't take.
    if (control === 'toggle') {
      // The position restarts from where it stands, so the progress line doesn't jump.
      const at = current === shown.value ? positionMs.value : current.positionMs;
      track.value = {
        ...current,
        playing: !current.playing,
        positionMs: at,
        updatedAt: Date.now(),
        lastPlayedAt: Date.now(),
      };
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

  return { track, shown, positionMs, press, focus, pause, resume };
}
