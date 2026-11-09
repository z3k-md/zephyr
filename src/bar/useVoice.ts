// Hold-to-talk. Rust owns the mic and the model; this shows the words as they arrive
// and, on release, runs them the way the matching key would.

import { nextTick, onMounted, onUnmounted, ref, type Ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { errorMessage, openUrl, voiceCancel } from '../api';
import type {
  SuggestionRow,
  SuggestResponse,
  VoiceErrorPayload,
  VoiceFinalPayload,
  VoiceLevelPayload,
  VoiceStatePayload,
  VoiceTextPayload,
} from '../types';
import { googleUrl } from './google';
import { parseVoice, type VoiceAction } from './voice';

export type VoicePhase = 'idle' | 'listening' | 'finishing';

const HINTS: Record<VoiceAction, string> = {
  type: '',
  top: 'Open',
  search: 'Google',
  ask: 'AI Mode',
  cancel: 'Cancel',
};

const RESULT_WAIT_MS = 1500;

/** Keys a shortcut token can arrive as. */
const KEY_NAMES: Record<string, string[]> = {
  ctrl: ['control'],
  control: ['control'],
  alt: ['alt'],
  option: ['alt'],
  shift: ['shift'],
  meta: ['meta'],
  cmd: ['meta'],
  command: ['meta'],
  super: ['meta'],
  win: ['meta'],
  space: [' ', 'space'],
  enter: ['enter'],
  esc: ['escape'],
  escape: ['escape'],
};

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

export function useVoice(ctx: {
  query: Ref<string>;
  scope: { value: unknown };
  view: { value: string };
  notice: Ref<string | null>;
  selected: Ref<number>;
  lastQuery: Ref<string>;
  /** The text the last applied suggest reply was fetched for (`typedText()` at the time). */
  lastTyped: Ref<string>;
  lastResponse: Ref<SuggestResponse | null>;
  typedText: () => string;
  items: () => SuggestionRow[];
  /** The saved shortcut, or the default while settings haven't loaded. */
  shortcut: () => string;
  /** True while the box holds words from the mic, so traces omit them. */
  redact: Ref<boolean>;
  emojiOpen: () => boolean;
  /** Enter on the view that's up (shell, clipboard, emoji). False if it isn't mounted yet. */
  pressEnter: () => boolean;
  accept: () => Promise<void>;
  googleAiMode: () => Promise<void>;
  /** Esc from the root: drop a parked workspace and hide. */
  close: () => void;
  /** Clears the main bar's text, scope, selection and notice. */
  reset: () => void;
  /** Clears the text once a page has opened, as a successful Enter does. */
  clear: () => void;
  /** The bar just started listening; drop a move-cursor left by the held modifier. */
  onListen: () => void;
}) {
  const phase = ref<VoicePhase>('idle');
  const level = ref(0);
  const modelReady = ref(true);
  const action = ref<VoiceAction>('top');
  const hint = ref(HINTS.top);
  /** Bumped on each hold and on Esc, so a final from an older hold doesn't run. */
  let session = 0;
  /** Esc asked to drop the final that's about to arrive. */
  let dropFinal = false;
  /** A final is being applied, so a trailing `idle` event doesn't hide the hint early. */
  let settling = false;

  function setPhase(next: VoicePhase) {
    phase.value = next;
    ctx.redact.value = next !== 'idle';
    if (next !== 'listening') level.value = 0;
  }

  function matchesShortcut(event: KeyboardEvent): boolean {
    const shortcut = ctx.shortcut();
    if (!shortcut) return false;
    const key = event.key.toLowerCase();
    const code = event.code.toLowerCase();
    return shortcut.split('+').some((part) => {
      const name = part.trim().toLowerCase();
      const aliases = KEY_NAMES[name];
      if (aliases) return aliases.includes(key) || aliases.includes(code);
      return key === name || code === `key${name}` || code === `digit${name}`;
    });
  }

  /** Swallow the held hotkey, and Esc cancels the hold. True when the bar should ignore it. */
  function onKey(event: KeyboardEvent): boolean {
    if (phase.value === 'idle') return false;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!event.repeat) {
        dropFinal = true;
        session += 1;
        settling = false;
        if (phase.value === 'listening') void voiceCancel().catch(() => undefined);
        setPhase('idle');
      }
      return true;
    }
    if (matchesShortcut(event)) {
      event.preventDefault();
      event.stopImmediatePropagation();
      return true;
    }
    return false;
  }

  function show(text: string) {
    const parsed = parseVoice(text);
    action.value = parsed.action;
    hint.value = HINTS[parsed.action];
    // Clear the scope in the same turn as the text, so a bang is parsed again
    // instead of being typed in front of a chip that's already showing.
    ctx.scope.value = null;
    ctx.query.value = parsed.query;
  }

  function begin(ready: boolean) {
    session += 1;
    dropFinal = false;
    settling = false;
    action.value = 'type';
    hint.value = HINTS.type;
    modelReady.value = ready;
    ctx.view.value = 'root';
    ctx.reset();
    setPhase('listening');
    ctx.onListen();
  }

  async function waitForResults() {
    const deadline = performance.now() + RESULT_WAIT_MS;
    while (performance.now() < deadline) {
      if (ctx.lastTyped.value === ctx.typedText() && ctx.lastQuery.value === ctx.query.value) {
        return;
      }
      await delay(30);
    }
  }

  async function runSearch() {
    const rows = ctx.items();
    const index = rows.findIndex((row) => row.kind === 'query' && row.target === 'web');
    if (index >= 0) {
      ctx.selected.value = index;
      await ctx.accept();
      return;
    }
    const text = ctx.query.value.trim();
    if (!text) return;
    try {
      await openUrl(googleUrl(text));
      ctx.clear();
    } catch (error) {
      ctx.notice.value = errorMessage(error);
    }
  }

  async function runAsk() {
    const rows = ctx.items();
    const index = rows.findIndex((row) => row.kind === 'query' && row.target === 'web');
    if (index >= 0) ctx.selected.value = index;
    else {
      const current = ctx.selected.value >= 0 ? rows[ctx.selected.value] : undefined;
      const searchable =
        !!current &&
        (current.kind === 'query' || current.kind === 'history' || current.kind === 'remote') &&
        current.target === 'web';
      if (!searchable) ctx.selected.value = -1;
    }
    if (index >= 0 || ctx.lastResponse.value?.target === 'web') {
      await ctx.googleAiMode();
      return;
    }
    const text = ctx.query.value.trim();
    if (!text) return;
    try {
      await openUrl(`${googleUrl(text)}&udm=50`);
      ctx.clear();
    } catch (error) {
      ctx.notice.value = errorMessage(error);
    }
  }

  /** Enter on a view the query already opened (the shell, clipboard, emoji). */
  async function runViewEnter(mine: number) {
    const deadline = performance.now() + RESULT_WAIT_MS;
    while (performance.now() < deadline) {
      if (session !== mine) return;
      if (ctx.pressEnter()) return;
      await delay(40);
    }
  }

  async function onFinal(payload: VoiceFinalPayload) {
    const mine = session;
    if (dropFinal) {
      dropFinal = false;
      setPhase('idle');
      return;
    }
    settling = true;
    try {
      if (payload.cancelled || !payload.text.trim()) return;
      const parsed = parseVoice(payload.text);
      action.value = parsed.action;
      hint.value = HINTS[parsed.action];
      if (parsed.action === 'cancel') {
        ctx.reset();
        ctx.close();
        return;
      }
      if (!parsed.query.trim()) return;
      setPhase('finishing');
      show(payload.text);
      // Without a command word the words wait in the box for Enter, like typed text.
      if (parsed.action === 'type') return;
      // Let the query watch turn a bang into a scope, or open the shell or clipboard.
      await nextTick();
      if (session !== mine) return;
      // An alias (or similar) already took the text and cleared the box.
      if (!ctx.query.value && ctx.view.value === 'root' && !ctx.emojiOpen()) return;
      if (parsed.action === 'top' && (ctx.view.value !== 'root' || ctx.emojiOpen())) {
        await runViewEnter(mine);
        return;
      }
      if (ctx.view.value !== 'root') return;
      await waitForResults();
      if (session !== mine) return;
      if (parsed.action === 'search') await runSearch();
      else if (parsed.action === 'ask') await runAsk();
      else await ctx.accept();
    } finally {
      if (session === mine) {
        settling = false;
        setPhase('idle');
      }
    }
  }

  let unlistens: UnlistenFn[] = [];
  onMounted(async () => {
    unlistens = await Promise.all([
      listen<VoiceStatePayload>('voice-state', (event) => {
        const { state, modelReady: ready } = event.payload;
        modelReady.value = ready;
        // Rust repeats `listening` mid-hold once the model has loaded.
        if (state === 'listening' && phase.value !== 'listening') begin(ready);
        else if (state === 'finishing') {
          if (phase.value !== 'idle') setPhase('finishing');
        } else if (!settling && phase.value !== 'finishing') {
          setPhase('idle');
        }
      }),
      listen<VoiceLevelPayload>('voice-level', (event) => {
        if (phase.value !== 'listening') return;
        const value = event.payload.level;
        level.value = Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
      }),
      listen<VoiceTextPayload>('voice-partial', (event) => {
        if (phase.value === 'idle' || settling) return;
        show(event.payload.text);
      }),
      listen<VoiceFinalPayload>('voice-final', (event) => {
        void onFinal(event.payload);
      }),
      listen<VoiceErrorPayload>('voice-error', (event) => {
        ctx.notice.value = event.payload.message;
      }),
    ]);
  });
  onUnmounted(() => {
    for (const unlisten of unlistens) unlisten();
  });

  return { phase, level, modelReady, hint, onKey, matchesShortcut };
}
