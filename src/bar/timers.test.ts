import { describe, expect, it } from 'vitest';
import { parseDuration, parseTimer, timeLeft, timerLength } from './timers';

describe('parseDuration', () => {
  it.each([
    ['120', 120],
    ['5m', 300],
    ['5 min', 300],
    ['10 minutes', 600],
    ['1h30m', 5400],
    ['1h 30m', 5400],
    ['1.5h', 5400],
    ['90s', 90],
    ['2m 15s', 135],
    ['2:30', 150],
    ['1:02:03', 3723],
  ])('%s is %i seconds', (text, seconds) => {
    expect(parseDuration(text)).toBe(seconds);
  });

  it.each(['', 'tea', '5x', 'm5', '2:75'])('%s is not a length', (text) => {
    expect(parseDuration(text)).toBeNull();
  });
});

describe('parseTimer', () => {
  it('offers presets for a bare "timer"', () => {
    expect(parseTimer('timer')).toEqual({ seconds: null, label: '' });
    expect(parseTimer('Timer ')).toEqual({ seconds: null, label: '' });
  });

  it('reads a bare number as seconds and units as given', () => {
    expect(parseTimer('timer 120')).toEqual({ seconds: 120, label: '' });
    expect(parseTimer('timer 5m')).toEqual({ seconds: 300, label: '' });
  });

  it('names the timer with the words after its length', () => {
    expect(parseTimer('timer 1h 30m pizza in the oven')).toEqual({
      seconds: 5400,
      label: 'pizza in the oven',
    });
    expect(parseTimer('timer 4 min tea')).toEqual({ seconds: 240, label: 'tea' });
  });

  it('leaves other text alone', () => {
    expect(parseTimer('timers')).toBeNull();
    expect(parseTimer('timer tea')).toBeNull();
    expect(parseTimer('timer 0')).toBeNull();
    expect(parseTimer('timer 30d')).toBeNull();
    expect(parseTimer('set timer 5m')).toBeNull();
  });
});

describe('timer text', () => {
  it('names lengths', () => {
    expect(timerLength(300)).toBe('5 minutes');
    expect(timerLength(60)).toBe('1 minute');
    expect(timerLength(45)).toBe('45 seconds');
    expect(timerLength(135)).toBe('2m 15s');
    expect(timerLength(5400)).toBe('1h 30m');
  });

  it('counts down on a clock face', () => {
    expect(timeLeft(245)).toBe('4:05');
    expect(timeLeft(3729)).toBe('1:02:09');
    expect(timeLeft(0.4)).toBe('0:01');
    expect(timeLeft(-3)).toBe('0:00');
  });
});
