import { describe, expect, it } from 'vitest';
import { captureShortcut, isMac, shortcutKeys } from './keys';

function press(key: string, mods: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    key,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...mods,
  } as KeyboardEvent;
}

describe('captureShortcut', () => {
  it.each([
    [press('k', { ctrlKey: true }), 'ctrl+k'],
    [press('K', { ctrlKey: true, shiftKey: true }), 'ctrl+shift+k'],
    [press(' ', { altKey: true }), 'alt+space'],
    [press('v', { metaKey: true, altKey: true }), 'alt+super+v'],
    [press('1', { ctrlKey: true }), 'ctrl+1'],
    [press('k'), null],
    [press('Control', { ctrlKey: true }), null],
    [press('Enter', { ctrlKey: true }), null],
    [press('F5', { ctrlKey: true }), null],
  ])('%#: %s', (event, expected) => {
    expect(captureShortcut(event)).toBe(expected);
  });

  it('takes function keys when asked', () => {
    expect(captureShortcut(press('F5', { ctrlKey: true }), true)).toBe('ctrl+f5');
  });
});

describe('shortcutKeys', () => {
  const cases: [string, string[]][] = isMac
    ? [
        ['mod+k', ['⌘', 'K']],
        ['command+shift+v', ['⇧', '⌘', 'V']],
        ['ctrl+alt+space', ['⌃', '⌥', 'Space']],
        ['enter', ['↵']],
      ]
    : [
        ['mod+k', ['Ctrl', 'K']],
        ['ctrl+shift+v', ['Ctrl', 'Shift', 'V']],
        ['alt+space', ['Alt', 'Space']],
        ['super+e', ['Win', 'E']],
        ['shift+enter', ['Shift', '↵']],
        ['KeyA+Digit1', ['A', '1']],
        ['up+down+esc', ['↑', '↓', 'Esc']],
      ];

  it.each(cases)('%s', (spec, keys) => {
    expect(shortcutKeys(spec)).toEqual(keys);
  });

  it('skips empty parts', () => {
    expect(shortcutKeys('k++')).toEqual(['K']);
  });
});
