import { describe, expect, it } from 'vitest';
import { colorFor, contrastAgainst, formatsFor } from './color';

const red = { r: 255, g: 0, b: 0, a: 1 };

describe('colorFor', () => {
  it.each([
    ['#f00', red],
    ['#F00', red],
    ['#ff0000', red],
    ['#FF0000', red],
    ['rgb(255, 0, 0)', red],
    ['rgb(255 0 0)', red],
    ['rgba(255, 0, 0, 1)', red],
    ['rgb(100% 0% 0%)', red],
    ['hsl(0, 100%, 50%)', red],
    ['hsl(0 100% 50%)', red],
    ['hsla(0, 100%, 50%, 1)', red],
    ['hwb(0 0% 0%)', red],
    ['color red', red],
    ['RED color', red],
    ['  color   red  ', red],
  ])('reads %s', (input, rgba) => {
    expect(colorFor(input)?.rgba).toEqual(rgba);
  });

  it('reads short and long hex alpha', () => {
    expect(colorFor('#f008')?.rgba).toEqual({ r: 255, g: 0, b: 0, a: 0x88 / 255 });
    expect(colorFor('#ff000080')?.rgba.a).toBeCloseTo(128 / 255, 5);
  });

  it('reads slash alpha and percent alpha', () => {
    expect(colorFor('rgb(255 0 0 / 0.5)')?.rgba.a).toBe(0.5);
    expect(colorFor('rgb(255, 0, 0, 50%)')?.rgba.a).toBe(0.5);
    expect(colorFor('hsl(120 100% 50% / 25%)')?.rgba).toMatchObject({ r: 0, g: 255, b: 0 });
  });

  it('reads oklch and oklab that land on red', () => {
    const fromLch = colorFor('oklch(0.627955 0.257683 29.233885)');
    const fromLab = colorFor('oklab(0.627955 0.224863 0.125846)');
    expect(fromLch?.rgba).toEqual(red);
    expect(fromLab?.rgba).toEqual(red);
  });

  it.each([
    'ff0000',
    '255',
    'orange',
    'red',
    'color',
    'color not-a-color',
    'rgb(255, 0)',
    '#ff',
    '#fffffg',
    '',
  ])('leaves %s alone', (input) => {
    expect(colorFor(input)).toBeNull();
  });

  it('formats as modern CSS', () => {
    expect(formatsFor(red)).toEqual({
      hex: '#ff0000',
      rgb: 'rgb(255 0 0)',
      hsl: 'hsl(0 100% 50%)',
      oklch: expect.stringMatching(/^oklch\(0\.628 0\.258 29\./),
    });
    const faded = colorFor('rgb(255 0 0 / 0.5)')!;
    expect(faded.formats.hex).toBe('#ff000080');
    expect(faded.formats.rgb).toBe('rgb(255 0 0 / 0.5)');
    expect(faded.formats.hsl).toBe('hsl(0 100% 50% / 0.5)');
    expect(faded.formats.oklch).toMatch(/ \/ 0\.5\)$/);
  });

  it('gives WCAG contrast against white and black', () => {
    const black = colorFor('#000')!;
    const white = colorFor('#fff')!;
    expect(black.contrastWhite).toBe(21);
    expect(black.contrastBlack).toBe(1);
    expect(white.contrastBlack).toBe(21);
    expect(white.contrastWhite).toBe(1);
    expect(contrastAgainst(red, 'white')).toBeGreaterThan(3);
    expect(contrastAgainst(red, 'white')).toBeLessThan(5);
  });

  it('makes seven shades and keeps the typed color as one of them', () => {
    const color = colorFor('#c026d3')!;
    expect(color.shades).toHaveLength(7);
    expect(color.shades[color.shadeIndex]).toEqual(color.rgba);
    for (const shade of color.shades) {
      expect(shade.r).toBeGreaterThanOrEqual(0);
      expect(shade.r).toBeLessThanOrEqual(255);
      expect(shade.g).toBeGreaterThanOrEqual(0);
      expect(shade.b).toBeLessThanOrEqual(255);
      expect(shade.a).toBe(1);
    }
    const lights = color.shades.map((shade) => shade.r + shade.g + shade.b);
    expect(lights[0]).toBeGreaterThan(lights[lights.length - 1]);
  });

  it('reduces chroma so a wide-gamut oklch still fits in sRGB', () => {
    const color = colorFor('oklch(0.7 0.4 30)')!;
    expect(color.rgba.r).toBeLessThanOrEqual(255);
    expect(color.rgba.g).toBeGreaterThanOrEqual(0);
    expect(color.shades).toHaveLength(7);
  });
});
