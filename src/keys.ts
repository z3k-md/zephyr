// Key names as the bar shows them: symbols on a Mac, words on Windows and Linux.

export const isMac = navigator.userAgent.includes('Mac');

const MOD = isMac ? '⌘' : 'Ctrl';
export const ALT = isMac ? '⌥' : 'Alt';
export const CTRL = isMac ? '⌃' : 'Ctrl';

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
    case 'cmdorctrl':
    case 'commandorcontrol':
    case 'commandorctrl':
    case 'cmdorcontrol':
      return MOD;
    case 'space':
      return 'Space';
  }
  const key = /^key([a-z])$/i.exec(part);
  if (key) return key[1].toUpperCase();
  const digit = /^digit(\d)$/i.exec(part);
  if (digit) return digit[1];
  return part.charAt(0).toUpperCase() + part.slice(1);
}

/** "command+shift+v" as keycaps, modifiers in the platform's usual order. */
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
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return null;
  if (event.key === ' ' || event.key === 'Spacebar') parts.push('space');
  else if (functionKeys && /^F\d{1,2}$/.test(event.key)) parts.push(event.key.toLowerCase());
  else if (event.key.length === 1) parts.push(event.key.toLowerCase());
  else return null;
  return parts.join('+');
}
