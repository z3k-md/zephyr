// Type-ahead: the rest of what Enter would act on, drawn faintly after the cursor. Enter
// still sends only what was typed; Right arrow or End takes the completion.

import { computed, nextTick, ref, type Ref } from 'vue';
import type { Snapshot, SuggestionRow } from '../types';

export function useTypeAhead(ctx: {
  query: Ref<string>;
  inputEl: Ref<HTMLInputElement | null>;
  snapshot: Ref<Snapshot | null>;
  /** No completion in a scoped search. */
  scoped: () => boolean;
  items: Ref<SuggestionRow[]>;
  selectedItem: Ref<SuggestionRow | undefined>;
}) {
  const { query, inputEl, snapshot, items, selectedItem } = ctx;
  const caretAtEnd = ref(true);
  // Past searches, most used first, sorted once per history change rather than per key.
  const searchHistory = computed(() =>
    [...(snapshot.value?.history ?? [])]
      .filter((entry) => entry.destinationId === 'google')
      .sort((a, b) => b.uses - a.uses || b.lastUsed - a.lastUsed)
      .map((entry) => entry.query)
  );

  const ghost = computed(() => {
    const typed = query.value;
    if (!typed.trim() || !caretAtEnd.value) return '';
    if (ctx.scoped() || typed.startsWith('!') || typed.startsWith('>')) return '';
    const lower = typed.toLowerCase();
    const target = selectedItem.value;
    let candidates: string[];
    if (target?.kind === 'app') {
      candidates = [target.label];
    } else if (target?.kind === 'history' || target?.kind === 'remote') {
      candidates = [target.query];
    } else {
      const history = searchHistory.value;
      const remote = items.value
        .filter((item) => item.kind === 'history' || item.kind === 'remote')
        .map((item) => item.query);
      candidates = [...history, ...remote];
    }
    const match = candidates.find(
      (candidate) => candidate.length > typed.length && candidate.toLowerCase().startsWith(lower)
    );
    return match ? match.slice(typed.length) : '';
  });

  /** The completion only shows while the cursor sits at the end of text that fits the box. */
  function trackCaret() {
    const input = inputEl.value;
    if (!input) return;
    const end = input.value.length;
    caretAtEnd.value =
      input.selectionStart === end &&
      input.selectionEnd === end &&
      input.scrollWidth <= input.clientWidth;
  }

  /** Right arrow or End at the end of the text takes the completion; whether it did. */
  function onKey(event: KeyboardEvent): boolean {
    if (event.key !== 'ArrowRight' && event.key !== 'End') return false;
    if (!ghost.value || event.shiftKey || event.ctrlKey || event.metaKey || event.altKey) {
      return false;
    }
    event.preventDefault();
    query.value += ghost.value;
    void nextTick(() => {
      inputEl.value?.setSelectionRange(query.value.length, query.value.length);
      trackCaret();
    });
    return true;
  }

  return { ghost, trackCaret, onKey };
}
