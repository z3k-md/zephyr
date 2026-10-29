// Asking Rust for the bar's results as the text changes. Local results (apps, history) land
// on the keystroke; remote suggestions follow after a pause and only append, so the row
// Enter would take never shifts under the user.

import { onUnmounted, type Ref } from 'vue';
import { errorMessage, suggest } from '../api';
import type { PowerPreview, Suggestion, SuggestResponse } from '../types';
import { previewPower } from './usePower';

export function useSuggest(ctx: {
  /** What Rust sees: the text, behind its scope's bang when there is one. */
  text: () => string;
  /** The results shown now, whose web rows can stay up until the new text's arrive. */
  shown: () => Suggestion[];
  /** What the typed text would do as a power command; set by the local pass. */
  powerRow: Ref<PowerPreview | null>;
  /** Shows a reply that's still current. */
  apply: (response: SuggestResponse, remote: boolean, typed: string) => Promise<void> | void;
  notice: Ref<string | null>;
  trace: (kind: string, data: unknown) => void;
}) {
  const { powerRow, notice, trace } = ctx;
  let generation = 0;
  let remoteApplied = false;
  let suggestTimer = 0;
  /** The generation whose local reply said the network pass would add nothing. */
  let remoteSkip = 0;

  function scheduleSuggest() {
    window.clearTimeout(suggestTimer);
    const current = ++generation;
    remoteApplied = false;
    void loadSuggestions(current, false);
    suggestTimer = window.setTimeout(() => {
      // The local pass already said the network has nothing to add for this text.
      if (remoteSkip === current) return;
      void loadSuggestions(current, true);
    }, 120);
  }
  onUnmounted(() => window.clearTimeout(suggestTimer));

  /** Web suggestions by text, so going back to a text shows them at once. */
  const remembered = new Map<string, Suggestion[]>();

  /** The web rows to keep up until this text's own arrive: only ones that still match. */
  function stillMatching(typed: string): Suggestion[] {
    const prefix = typed.trim().toLowerCase();
    if (!prefix) return [];
    return ctx
      .shown()
      .filter((item) => item.kind === 'remote' && item.query.toLowerCase().startsWith(prefix));
  }

  async function loadSuggestions(current: number, includeRemote: boolean) {
    try {
      const typed = ctx.text();
      const asked = performance.now();
      trace('fetch', { text: typed, remote: includeRemote, generation: current });
      const [answer, power] = await Promise.all([
        suggest(typed, includeRemote),
        includeRemote ? Promise.resolve(powerRow.value) : previewPower(typed),
      ]);
      let response = answer;
      if (!includeRemote && current === generation) {
        powerRow.value = power;
        remoteSkip = answer.remote === false ? current : 0;
      }
      const stale = current !== generation || (!includeRemote && remoteApplied);
      trace('fetched', {
        text: typed,
        remote: includeRemote,
        ms: Math.round(performance.now() - asked),
        rows: response.items.length,
        stale,
      });
      if (stale) return;
      if (includeRemote) remoteApplied = true;
      // Until this text's web suggestions arrive, keep the ones seen for it before
      // (backspacing), or the current ones that still match, so the list doesn't collapse.
      const key = typed.trim().toLowerCase();
      if (includeRemote) {
        remembered.set(
          key,
          response.items.filter((item) => item.kind === 'remote')
        );
        if (remembered.size > 200) remembered.delete(remembered.keys().next().value ?? '');
      } else {
        // Not when no web suggestions will follow (an app named outright, a link).
        const carried =
          answer.remote === false ? [] : (remembered.get(key) ?? stillMatching(typed));
        if (carried.length) response = { ...response, items: [...response.items, ...carried] };
      }
      await ctx.apply(response, includeRemote, typed);
    } catch (error) {
      if (current !== generation) return;
      notice.value = errorMessage(error);
    }
  }

  return { scheduleSuggest };
}
