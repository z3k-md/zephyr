import { ref } from 'vue';
import { errorMessage, soundSetMuted, soundSetVolume } from '../api';

/** A level the mixer shows: the device's (`master`) or an app's. */
export interface MixerLevel {
  key: string;
  name: string;
  icon: string;
  volume: number;
  muted: boolean;
  /** How loud it is right now, 0–1, where it's measured. */
  peak?: number;
  active?: boolean;
}

/** A notch of the arrows and the wheel, as Windows steps the volume keys. */
export const STEP = 0.02;
/** A level just changed here keeps its new value this long, so a refresh already on its way
 *  doesn't pull the slider back. */
const HOLD_MS = 800;

export function percent(value: number): number {
  return Math.round(value * 100);
}

/** Setting levels from the Sound view or the bar's drawer: at once on screen, then on the
 *  system. A level set here shows over the system's for `holdMs`. */
export function useMixer(onError: (message: string) => void, holdMs = HOLD_MS) {
  /** Levels set here and not yet confirmed by a refresh, by key. */
  const held = ref<Record<string, { volume: number; muted: boolean; until: number }>>({});

  /** The rows with what was just set here in place of what the system last said. */
  function withHeld<T extends MixerLevel>(rows: T[]): T[] {
    const time = performance.now();
    return rows.map((row) => {
      const hold = held.value[row.key];
      return hold && hold.until > time ? { ...row, volume: hold.volume, muted: hold.muted } : row;
    });
  }

  async function apply(row: MixerLevel, volume: number, muted: boolean) {
    volume = Math.min(1, Math.max(0, Math.round(volume * 100) / 100));
    held.value = {
      ...held.value,
      [row.key]: { volume, muted, until: performance.now() + holdMs },
    };
    try {
      if (volume !== row.volume) await soundSetVolume(row.key, volume);
      if (muted !== row.muted) await soundSetMuted(row.key, muted);
    } catch (error) {
      onError(errorMessage(error));
    }
  }

  function nudge(row: MixerLevel, delta: number) {
    // Turning a muted row up unmutes it, as Windows' own slider does.
    void apply(row, row.volume + delta, row.muted && delta <= 0);
  }

  function toggleMute(row: MixerLevel) {
    void apply(row, row.volume, !row.muted);
  }

  /** The level under the pointer, while it presses or drags along a slider. `done` runs when
   *  the pointer lets go. */
  function slide(row: MixerLevel, event: PointerEvent, done?: () => void) {
    const track = event.currentTarget as HTMLElement;
    // The row the drag started on, so each move compares with the level it last set.
    let last: MixerLevel = row;
    const place = (move: PointerEvent) => {
      const box = track.getBoundingClientRect();
      const level = (move.clientX - box.left) / Math.max(1, box.width);
      void apply(last, level, false);
      last = { ...last, volume: Math.min(1, Math.max(0, level)), muted: false };
    };
    track.setPointerCapture(event.pointerId);
    place(event);
    const onMove = (move: PointerEvent) => place(move);
    const onUp = () => {
      track.removeEventListener('pointermove', onMove);
      track.removeEventListener('pointerup', onUp);
      track.removeEventListener('pointercancel', onUp);
      done?.();
    };
    track.addEventListener('pointermove', onMove);
    track.addEventListener('pointerup', onUp);
    track.addEventListener('pointercancel', onUp);
  }

  /** The wheel over a row: a notch is one step. */
  function wheel(row: MixerLevel, event: WheelEvent) {
    event.preventDefault();
    nudge(row, event.deltaY < 0 ? STEP : -STEP);
  }

  return { held, withHeld, apply, nudge, toggleMute, slide, wheel };
}
