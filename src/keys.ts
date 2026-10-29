// Key names as the bar shows them: symbols on a Mac, words on Windows and Linux.

export const isMac = navigator.userAgent.includes('Mac');

const MOD = isMac ? '⌘' : 'Ctrl';
const ALT = isMac ? '⌥' : 'Alt';
const CTRL = isMac ? '⌃' : 'Ctrl';
/** The command key as the start of a keycap hint: "⌘K" on a Mac, "Ctrl+K" elsewhere. */
export const MOD_KEY = isMac ? '⌘' : 'Ctrl+';

const ORDER = ['⌃', '⌥', '⇧', '⌘'];

function keyName(part: string): string {
  const lower = part.toLowerCase();
  switch (lower) {
    case 'ctrl':
    case 'control':
      return CTRL;
    case 'alt':
    case 'option':
      return ALT;
    case 'shift':
      return isMac ? '⇧' : 'Shift';
    case 'super':
    case 'command':
    case 'cmd':
    case 'meta':
      return isMac ? '⌘' : 'Win';
    case 'win':
      return isMac ? '⌘' : 'Win';
    case 'mod':
    case 'cmdorctrl':
    case 'commandorcontrol':
    case 'commandorctrl':
    case 'cmdorcontrol':
      return MOD;
    case 'space':
      return 'Space';
    case 'enter':
    case 'return':
      return '↵';
    case 'esc':
    case 'escape':
      return 'Esc';
    case 'tab':
      return 'Tab';
    case 'backspace':
      return '⌫';
    case 'delete':
    case 'del':
      return 'Del';
    case 'up':
    case 'arrowup':
      return '↑';
    case 'down':
    case 'arrowdown':
      return '↓';
    case 'left':
    case 'arrowleft':
      return '←';
    case 'right':
    case 'arrowright':
      return '→';
  }
  const key = /^key([a-z])$/i.exec(part);
  if (key) return key[1].toUpperCase();
  const digit = /^digit(\d)$/i.exec(part);
  if (digit) return digit[1];
  return part.charAt(0).toUpperCase() + part.slice(1);
}

/**
 * "command+shift+v" as keycaps, modifiers in the platform's usual order. Besides modifiers
 * (ctrl, alt, shift, super or win, and mod: ⌘ on a Mac, Ctrl elsewhere) a spec can name
 * enter, esc, tab, backspace, delete, space and the arrows (up, down, left, right).
 */
export function shortcutKeys(shortcut: string): string[] {
  const keys = shortcut
    .split('+')
    .filter((part) => part.length > 0)
    .map(keyName);
  if (!isMac) return keys;
  const modifiers = keys.filter((key) => ORDER.includes(key));
  const rest = keys.filter((key) => !ORDER.includes(key));
  modifiers.sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
  return [...modifiers, ...rest];
}

const SPOKEN: Record<string, string> = {
  '⌘': 'Command',
  '⌥': 'Option',
  '⌃': 'Control',
  '⇧': 'Shift',
  '↵': 'Enter',
  '⌫': 'Backspace',
  Esc: 'Escape',
  Del: 'Delete',
  '↑': 'Up',
  '↓': 'Down',
  '←': 'Left',
  '→': 'Right',
};

/** Keycaps as words for a screen reader: "Command+Enter" for ⌘ ↵. */
export function spokenKeys(keys: string[]): string {
  return keys.map((key) => SPOKEN[key] ?? key).join('+');
}

const SHORTCUT_LABELS: Record<string, string> = {
  ctrl: 'Ctrl',
  control: 'Ctrl',
  alt: 'Alt',
  option: 'Alt',
  shift: 'Shift',
  super: 'Command',
  command: 'Command',
  cmd: 'Command',
  meta: 'Command',
  space: 'Space',
};

/** "ctrl+shift+k" as words for Settings and setup: "Ctrl+Shift+K". */
export function formatShortcut(shortcut: string): string {
  return shortcut
    .split('+')
    .filter((part) => part.length > 0)
    .map(
      (part) => SHORTCUT_LABELS[part.toLowerCase()] ?? part.charAt(0).toUpperCase() + part.slice(1)
    )
    .join('+');
}

/** A modifier pressed on its own: a combo being recorded isn't finished yet. */
export function isModifierKey(event: KeyboardEvent): boolean {
  return ['Control', 'Alt', 'Shift', 'Meta'].includes(event.key);
}

/** What to tell someone whose combo `captureShortcut` turned down. */
export const SHORTCUT_HELP = 'Use at least one modifier and a letter, number, or space.';

/**
 * The modifiers held during `event`, as "ctrl+alt", for showing a shortcut while it's
 * being pressed; empty when none are.
 */
export function heldModifiers(event: KeyboardEvent): string {
  const parts: string[] = [];
  if (event.ctrlKey) parts.push('ctrl');
  if (event.altKey) parts.push('alt');
  if (event.shiftKey) parts.push('shift');
  if (event.metaKey) parts.push('super');
  return parts.join('+');
}

/**
 * A pressed shortcut as "ctrl+shift+k": at least one modifier and a letter, number, symbol
 * or space (function keys too when `functionKeys`); null while it's incomplete.
 */
export function captureShortcut(event: KeyboardEvent, functionKeys = false): string | null {
  const parts: string[] = [];
  if (event.ctrlKey) parts.push('ctrl');
  if (event.altKey) parts.push('alt');
  if (event.shiftKey) parts.push('shift');
  if (event.metaKey) parts.push('super');
  if (parts.length === 0) return null;
  if (isModifierKey(event)) return null;
  if (event.key === ' ' || event.key === 'Spacebar') parts.push('space');
  else if (functionKeys && /^F\d{1,2}$/.test(event.key)) parts.push(event.key.toLowerCase());
  else if (event.key.length === 1) parts.push(event.key.toLowerCase());
  else return null;
  return parts.join('+');
}
