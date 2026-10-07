// Key names as the bar shows them: symbols on a Mac, words on Windows and Linux.

export const isMac = navigator.userAgent.includes('Mac');

export const MOD = isMac ? '⌘' : 'Ctrl';
export const ALT = isMac ? '⌥' : 'Alt';
export const CTRL = isMac ? '⌃' : 'Ctrl';
export const ENTER = '↵';

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
