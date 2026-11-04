import { describe, expect, it } from 'vitest';
import { isMac } from '../../keys';
import raw from './data.json';
import { build, showsSubgroup } from './catalog';
import { withSkin } from './skin';

function entry(name: string) {
  const found = raw.emoji.find((item) => item.n === name);
  if (!found?.p) throw new Error(`${name} should take a skin tone`);
  return found;
}

describe('generated skin tones', () => {
  it('gives victory hand and writing hand a tone', () => {
    const victory = entry('victory hand');
    const writing = entry('writing hand');
    expect(victory.s).toBe(1);
    expect(writing.s).toBe(1);
    expect(withSkin(victory.c, 1, victory.p)).toBe('✌🏻');
    expect(withSkin(writing.c, 2, writing.p)).toBe('✍🏼');
  });

  it('tones both people holding hands, and a profession once', () => {
    const people = entry('people holding hands');
    const coder = entry('technologist');
    expect(withSkin(people.c, 1, people.p)).toBe('🧑🏻‍🤝‍🧑🏻');
    expect(withSkin(coder.c, 3, coder.p)).toBe('🧑🏽‍💻');
    const points = [...people.c].map((char) => char.codePointAt(0));
    expect(people.p.some((index) => points[index] === 0x1f91d)).toBe(false);
  });
});

describe('flag subgroups', () => {
  it('records country and subdivision flags and hides them off a Mac', () => {
    const subgroup = (name: string) => {
      const found = raw.emoji.find((item) => item.n === name);
      if (!found) throw new Error(name);
      return raw.subgroups[found.u];
    };
    expect(subgroup('flag: united states')).toBe('country-flag');
    expect(subgroup('flag: england')).toBe('subdivision-flag');
    expect(subgroup('chequered flag')).toBe('flag');
    expect(showsSubgroup('country-flag', false)).toBe(false);
    expect(showsSubgroup('subdivision-flag', false)).toBe(false);
    expect(showsSubgroup('country-flag', true)).toBe(true);
    expect(showsSubgroup('flag', false)).toBe(true);

    const names = new Set(build(raw).items.map((item) => item.name));
    expect(names.has('chequered flag')).toBe(true);
    expect(names.has('flag: united states')).toBe(isMac);
    expect(names.has('flag: england')).toBe(isMac);
  });
});
