// Power commands ("shutdown in 30m"): what the typed text would do, a pending schedule with
// its countdown, and the first Enter of a shutdown, restart or sign-out waiting for its
// second.

import { ref, type Ref } from 'vue';
import { errorMessage, powerCancel, powerPreview, powerRun } from '../api';
import { clock } from '../format';
import type { PowerPreview, Snapshot, SuggestionRow } from '../types';

const POWER_ICONS: Record<string, string> = {
  shutdown: 'glyph:power',
  restart: 'glyph:restart',
  sleep: 'glyph:moon',
  lock: 'glyph:lock',
  sign_out: 'glyph:signout',
  firmware: 'glyph:chip',
};
const POWER_LABELS: Record<string, string> = {
  shutdown: 'Shut down',
  restart: 'Restart',
  sleep: 'Sleep',
  lock: 'Lock',
  sign_out: 'Sign out',
  firmware: 'Restart to BIOS',
};
const POWER_NAMES: Record<string, string> = {
  shutdown: 'Shutting down',
  restart: 'Restarting',
  sleep: 'Sleeping',
  lock: 'Locking',
  sign_out: 'Signing out',
  firmware: 'Restarting to BIOS',
};

/** Text that could be a power command (power.rs WORDS, three letters in), so other text
 *  skips asking. */
const POWER_WORD =
  /^!?\s*(?:(?:cancel|abort|stop)\s+)?(?:res|reb|boo|fir|bio|uef|shu|pow|tur|sle|sus|loc|sig|log)/i;

/** What `typed` would do as a power command, if it reads like one. */
export function previewPower(typed: string): Promise<PowerPreview | null> {
  return POWER_WORD.test(typed) ? powerPreview(typed).catch(() => null) : Promise.resolve(null);
}

function untilText(at: number): string {
  const left = Math.max(0, at - Math.floor(clock.value / 1000));
  // Rounded up to the minute first, so 1h 59m 30s reads "2h", not "1h 60m".
  const total = Math.ceil(left / 60);
  const hours = Math.floor(total / 60);
  const minutes = total % 60;
  const span = hours ? `${hours}h${minutes ? ` ${minutes}m` : ''}` : `${minutes || 1}m`;
  if (left < 3600) return `in ${span}`;
  const time = new Date(at * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  return `at ${time}, in ${span}`;
}

export function usePower(ctx: {
  snapshot: Ref<Snapshot | null>;
  query: Ref<string>;
  notice: Ref<string | null>;
}) {
  const { snapshot, query, notice } = ctx;
  /** What the typed text would do. */
  const powerRow = ref<PowerPreview | null>(null);
  /** A command waiting for its second Enter (a power action, or a port's quit). */
  const confirmArm = ref('');

  function powerRows(): SuggestionRow[] {
    const rows: SuggestionRow[] = [];
    const pending = snapshot.value?.power;
    if (pending) {
      rows.push({
        kind: 'powerPending',
        label: `${POWER_NAMES[pending.action] ?? 'Scheduled'} ${untilText(pending.at)}`,
        query: '',
        target: '',
        hint: 'Enter cancels',
        keyHints: [{ keys: 'enter', label: 'cancels' }],
        section: 'power',
        icon: POWER_ICONS[pending.action] ?? 'glyph:power',
      });
    }
    const preview = powerRow.value;
    if (preview && !(preview.cancel && !pending)) {
      const key = `${preview.action}:${preview.at ?? 'now'}`;
      rows.push({
        kind: 'power',
        label: preview.label,
        query: '',
        target: '',
        hint:
          confirmArm.value === key
            ? 'Enter again to confirm'
            : preview.cancel
              ? 'Cancel'
              : preview.at
                ? 'Schedule'
                : 'Run',
        keyHints:
          confirmArm.value === key ? [{ keys: 'enter', label: 'again to confirm' }] : undefined,
        section: 'power',
        icon: POWER_ICONS[preview.action] ?? 'glyph:power',
        power: preview,
      });
      // Under a plain "shutdown", a few timers, so it's clear they exist. They're cancelable
      // from the top of the bar, so they skip the second Enter.
      if (!preview.at && !preview.cancel && preview.action !== 'lock') {
        const now = Math.floor(Date.now() / 1000);
        for (const [seconds, words] of [
          [300, '5 minutes'],
          [900, '15 minutes'],
          [3600, '1 hour'],
        ] as const) {
          rows.push({
            kind: 'power',
            label: `${POWER_LABELS[preview.action]} in ${words}`,
            query: '',
            target: '',
            hint: 'Schedule',
            section: 'power',
            icon: 'glyph:clock',
            power: {
              ...preview,
              label: `${POWER_LABELS[preview.action]} in ${words}`,
              at: now + seconds,
              confirm: false,
              exact: false,
            },
          });
        }
      }
    }
    return rows;
  }

  /** Shut down, restart and sign out take a second Enter; sleep and lock go at once. */
  async function runPower(preview: PowerPreview) {
    const key = `${preview.action}:${preview.at ?? 'now'}`;
    if (preview.confirm && !preview.at && confirmArm.value !== key) {
      confirmArm.value = key;
      notice.value = `Press Enter again to ${preview.label.toLowerCase()}, or Esc to back out`;
      return;
    }
    confirmArm.value = '';
    try {
      await powerRun(preview.action, preview.at);
      query.value = '';
      notice.value = preview.at ? `${preview.label}. Open Zephyr to cancel it.` : null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function cancelPower() {
    try {
      await powerCancel();
      query.value = '';
      notice.value = 'Cancelled';
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  return { powerRow, confirmArm, powerRows, runPower, cancelPower };
}
