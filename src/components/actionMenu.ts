// The ⌘K menu (Ctrl+K elsewhere) the bar and its views share: what else the selected thing
// can do, picked with the arrows and Enter. Lists never scroll, so the arrows stay among the
// actions that fit (see v-fit).

import { ref, toValue, type MaybeRefOrGetter } from 'vue';
import { isModifierKey } from '../keys';

export interface MenuAction {
  label: string;
  /** Its key, as a spec for the Keys component ("shift+enter"). */
  keys?: string;
  /** The menu stays open after it runs. */
  keepOpen?: boolean;
  /** Why it can't be done here; shown greyed with the reason. */
  disabled?: string;
  run: () => unknown;
}

/** The footer's hints while the menu is open. */
export const MENU_HINTS = [
  { keys: 'enter', label: 'run' },
  { keys: 'esc', label: 'back' },
];

/** ⌘K on a Mac, Ctrl+K elsewhere. */
export function isMenuKey(event: KeyboardEvent): boolean {
  return (
    (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.code === 'KeyK'
  );
}

export function useActionMenu<A extends MenuAction>(
  actions: MaybeRefOrGetter<A[]>,
  options: {
    /** While open, other keys leave the menu up and go on to the search box, instead of
     *  closing it and doing what they do in the list. */
    keepOpenWhileTyping?: boolean;
    /** Esc closed the menu. */
    onEscape?: () => void;
  } = {}
) {
  const open = ref(false);
  const index = ref(0);
  /** How many actions fit, reported by v-fit. */
  const shown = ref(Infinity);

  function close() {
    open.value = false;
  }

  /** Opens at the first action, if there's any; closes if open. */
  function toggle() {
    index.value = 0;
    open.value = !open.value && toValue(actions).length > 0;
  }

  function run(action: A | undefined) {
    if (!action || action.disabled) return;
    if (!action.keepOpen) open.value = false;
    void action.run();
  }

  function hover(at: number) {
    index.value = at;
  }

  /** Takes a key while the menu is open (and ⌘K at any time); false when the key is the
   *  view's to handle. */
  function onKey(event: KeyboardEvent): boolean {
    if (isMenuKey(event)) {
      event.preventDefault();
      toggle();
      return true;
    }
    if (!open.value) return false;
    if (event.key === 'Escape') {
      event.preventDefault();
      open.value = false;
      options.onEscape?.();
      return true;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const count = Math.min(toValue(actions).length, shown.value);
      if (count > 0) {
        index.value = (index.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
      }
      return true;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (!event.repeat) run(toValue(actions)[index.value]);
      return true;
    }
    if (options.keepOpenWhileTyping || isModifierKey(event)) return true;
    // Any other key closes the menu and does what it does in the list.
    open.value = false;
    return false;
  }

  return { open, index, shown, close, toggle, run, hover, onKey };
}
