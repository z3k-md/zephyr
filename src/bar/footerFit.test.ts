import { describe, expect, it } from 'vitest';
import {
  footerFit,
  footerRoom,
  footerWidth,
  type FooterFit,
  type FooterPresence,
} from './footerFit';

const crowded: FooterPresence = {
  playing: true,
  focus: true,
  meters: true,
  gpu: true,
  disk: true,
  net: true,
  meeting: true,
  weather: true,
};

describe('footerFit', () => {
  it('keeps a quiet footer whole', () => {
    expect(
      footerFit({
        ...crowded,
        playing: false,
        focus: false,
        gpu: false,
        disk: false,
        net: false,
        meeting: false,
      })
    ).toBe(0);
    expect(
      footerFit({
        ...crowded,
        playing: false,
        focus: false,
        gpu: false,
        disk: false,
        net: false,
        meeting: false,
        weather: false,
        meters: false,
      })
    ).toBe(0);
  });

  it('gives way in order until the crowded footer fits', () => {
    const level = footerFit(crowded);
    const room = footerRoom(true);
    expect(footerWidth(crowded, 0)).toBeGreaterThan(room);
    expect(footerWidth(crowded, level)).toBeLessThanOrEqual(room);
    if (level > 0) {
      expect(footerWidth(crowded, (level - 1) as FooterFit)).toBeGreaterThan(room);
    }
    // Now playing, focus, CPU/GPU/memory, the disk warning, the meeting and the
    // weather (and a network chip) need every step, through shortening the song.
    expect(level).toBe(4);
  });

  it('never gets more crowded by removing a piece', () => {
    const full = footerFit(crowded);
    for (const key of Object.keys(crowded) as (keyof FooterPresence)[]) {
      expect(footerFit({ ...crowded, [key]: false })).toBeLessThanOrEqual(full);
    }
  });
});
