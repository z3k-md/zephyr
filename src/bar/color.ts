// A color typed in the bar: hex, a CSS color function, or "color red" / "red color".
// A bare name ("orange", "red") is left for the app search. The # on a hex is required,
// so a plain number stays a calculation.

export interface Rgba {
  /** 0–255. */
  r: number;
  g: number;
  b: number;
  /** 0–1. */
  a: number;
}

export interface ColorFormats {
  hex: string;
  rgb: string;
  hsl: string;
  oklch: string;
}

export interface ColorAnswer {
  rgba: Rgba;
  formats: ColorFormats;
  /** WCAG contrast against white and against black, the color drawn over each. */
  contrastWhite: number;
  contrastBlack: number;
  /** Seven shades, light to dark, the typed color among them. */
  shades: Rgba[];
  shadeIndex: number;
}

const WHITE: Rgba = { r: 255, g: 255, b: 255, a: 1 };
const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 1 };

/** Lightness steps for the shade strip, light to dark. One is replaced by the typed color. */
const SHADE_LIGHTNESS = [0.97, 0.84, 0.71, 0.58, 0.45, 0.32, 0.18];

/** CSS named colors. `transparent` is handled beside this, since it has no color of its own. */
const NAMED: Record<string, number> = {
  aliceblue: 0xf0f8ff,
  antiquewhite: 0xfaebd7,
  aqua: 0x00ffff,
  aquamarine: 0x7fffd4,
  azure: 0xf0ffff,
  beige: 0xf5f5dc,
  bisque: 0xffe4c4,
  black: 0x000000,
  blanchedalmond: 0xffebcd,
  blue: 0x0000ff,
  blueviolet: 0x8a2be2,
  brown: 0xa52a2a,
  burlywood: 0xdeb887,
  cadetblue: 0x5f9ea0,
  chartreuse: 0x7fff00,
  chocolate: 0xd2691e,
  coral: 0xff7f50,
  cornflowerblue: 0x6495ed,
  cornsilk: 0xfff8dc,
  crimson: 0xdc143c,
  cyan: 0x00ffff,
  darkblue: 0x00008b,
  darkcyan: 0x008b8b,
  darkgoldenrod: 0xb8860b,
  darkgray: 0xa9a9a9,
  darkgreen: 0x006400,
  darkgrey: 0xa9a9a9,
  darkkhaki: 0xbdb76b,
  darkmagenta: 0x8b008b,
  darkolivegreen: 0x556b2f,
  darkorange: 0xff8c00,
  darkorchid: 0x9932cc,
  darkred: 0x8b0000,
  darksalmon: 0xe9967a,
  darkseagreen: 0x8fbc8f,
  darkslateblue: 0x483d8b,
  darkslategray: 0x2f4f4f,
  darkslategrey: 0x2f4f4f,
  darkturquoise: 0x00ced1,
  darkviolet: 0x9400d3,
  deeppink: 0xff1493,
  deepskyblue: 0x00bfff,
  dimgray: 0x696969,
  dimgrey: 0x696969,
  dodgerblue: 0x1e90ff,
  firebrick: 0xb22222,
  floralwhite: 0xfffaf0,
  forestgreen: 0x228b22,
  fuchsia: 0xff00ff,
  gainsboro: 0xdcdcdc,
  ghostwhite: 0xf8f8ff,
  gold: 0xffd700,
  goldenrod: 0xdaa520,
  gray: 0x808080,
  green: 0x008000,
  greenyellow: 0xadff2f,
  grey: 0x808080,
  honeydew: 0xf0fff0,
  hotpink: 0xff69b4,
  indianred: 0xcd5c5c,
  indigo: 0x4b0082,
  ivory: 0xfffff0,
  khaki: 0xf0e68c,
  lavender: 0xe6e6fa,
  lavenderblush: 0xfff0f5,
  lawngreen: 0x7cfc00,
  lemonchiffon: 0xfffacd,
  lightblue: 0xadd8e6,
  lightcoral: 0xf08080,
  lightcyan: 0xe0ffff,
  lightgoldenrodyellow: 0xfafad2,
  lightgray: 0xd3d3d3,
  lightgreen: 0x90ee90,
  lightgrey: 0xd3d3d3,
  lightpink: 0xffb6c1,
  lightsalmon: 0xffa07a,
  lightseagreen: 0x20b2aa,
  lightskyblue: 0x87cefa,
  lightslategray: 0x778899,
  lightslategrey: 0x778899,
  lightsteelblue: 0xb0c4de,
  lightyellow: 0xffffe0,
  lime: 0x00ff00,
  limegreen: 0x32cd32,
  linen: 0xfaf0e6,
  magenta: 0xff00ff,
  maroon: 0x800000,
  mediumaquamarine: 0x66cdaa,
  mediumblue: 0x0000cd,
  mediumorchid: 0xba55d3,
  mediumpurple: 0x9370db,
  mediumseagreen: 0x3cb371,
  mediumslateblue: 0x7b68ee,
  mediumspringgreen: 0x00fa9a,
  mediumturquoise: 0x48d1cc,
  mediumvioletred: 0xc71585,
  midnightblue: 0x191970,
  mintcream: 0xf5fffa,
  mistyrose: 0xffe4e1,
  moccasin: 0xffe4b5,
  navajowhite: 0xffdead,
  navy: 0x000080,
  oldlace: 0xfdf5e6,
  olive: 0x808000,
  olivedrab: 0x6b8e23,
  orange: 0xffa500,
  orangered: 0xff4500,
  orchid: 0xda70d6,
  palegoldenrod: 0xeee8aa,
  palegreen: 0x98fb98,
  paleturquoise: 0xafeeee,
  palevioletred: 0xdb7093,
  papayawhip: 0xffefd5,
  peachpuff: 0xffdab9,
  peru: 0xcd853f,
  pink: 0xffc0cb,
  plum: 0xdda0dd,
  powderblue: 0xb0e0e6,
  purple: 0x800080,
  rebeccapurple: 0x663399,
  red: 0xff0000,
  rosybrown: 0xbc8f8f,
  royalblue: 0x4169e1,
  saddlebrown: 0x8b4513,
  salmon: 0xfa8072,
  sandybrown: 0xf4a460,
  seagreen: 0x2e8b57,
  seashell: 0xfff5ee,
  sienna: 0xa0522d,
  silver: 0xc0c0c0,
  skyblue: 0x87ceeb,
  slateblue: 0x6a5acd,
  slategray: 0x708090,
  slategrey: 0x708090,
  snow: 0xfffafa,
  springgreen: 0x00ff7f,
  steelblue: 0x4682b4,
  tan: 0xd2b48c,
  teal: 0x008080,
  thistle: 0xd8bfd8,
  tomato: 0xff6347,
  turquoise: 0x40e0d0,
  violet: 0xee82ee,
  wheat: 0xf5deb3,
  white: 0xffffff,
  whitesmoke: 0xf5f5f5,
  yellow: 0xffff00,
  yellowgreen: 0x9acd32,
};

/** The color `input` names, or null when it isn't one. */
export function colorFor(input: string): ColorAnswer | null {
  const rgba = parseColor(input);
  if (!rgba) return null;
  const { shades, index } = shadesFor(rgba);
  return {
    rgba,
    formats: formatsFor(rgba),
    contrastWhite: contrast(rgba, WHITE),
    contrastBlack: contrast(rgba, BLACK),
    shades,
    shadeIndex: index,
  };
}

export function formatsFor(rgba: Rgba): ColorFormats {
  return {
    hex: formatHex(rgba),
    rgb: formatRgb(rgba),
    hsl: formatHsl(rgba),
    oklch: formatOklch(rgba),
  };
}

/** WCAG contrast of `fg` drawn over white or black. */
export function contrastAgainst(fg: Rgba, background: 'white' | 'black'): number {
  return contrast(fg, background === 'white' ? WHITE : BLACK);
}

function parseColor(input: string): Rgba | null {
  const text = input.trim();
  if (!text || text.length > 200) return null;
  return parseHex(text) ?? parseFunction(text) ?? namedColor(text);
}

function parseHex(text: string): Rgba | null {
  const match = /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.exec(text);
  if (!match) return null;
  let hex = match[1];
  if (hex.length <= 4) hex = [...hex].map((ch) => ch + ch).join('');
  const n = Number.parseInt(hex, 16);
  if (hex.length === 6) {
    return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255, a: 1 };
  }
  return {
    r: (n >> 24) & 255,
    g: (n >> 16) & 255,
    b: (n >> 8) & 255,
    a: (n & 255) / 255,
  };
}

function namedColor(text: string): Rgba | null {
  const flat = text.toLowerCase().replace(/\s+/g, ' ');
  let name = '';
  if (flat.startsWith('color ')) name = flat.slice('color '.length).trim();
  else if (flat.endsWith(' color')) name = flat.slice(0, -' color'.length).trim();
  else return null;
  if (!name || name.includes(' ')) return null;
  if (name === 'transparent') return { r: 0, g: 0, b: 0, a: 0 };
  const hex = NAMED[name];
  if (hex === undefined) return null;
  return { r: (hex >> 16) & 255, g: (hex >> 8) & 255, b: hex & 255, a: 1 };
}

function parseFunction(text: string): Rgba | null {
  const match = /^(rgb|rgba|hsl|hsla|hwb|oklch|oklab)\((.*)\)$/is.exec(text);
  if (!match) return null;
  const split = splitArgs(match[2]);
  if (!split) return null;
  switch (match[1].toLowerCase()) {
    case 'rgb':
    case 'rgba':
      return parseRgb(split.parts, split.alpha);
    case 'hsl':
    case 'hsla':
      return parseHsl(split.parts, split.alpha);
    case 'hwb':
      return parseHwb(split.parts, split.alpha);
    case 'oklch':
      return parseOklch(split.parts, split.alpha);
    case 'oklab':
      return parseOklab(split.parts, split.alpha);
    default:
      return null;
  }
}

/** Comma or space syntax, with `/ alpha` pulled out. A second slash is not a color. */
function splitArgs(body: string): { parts: string[]; alpha: string | null } | null {
  const tokens = body.trim().replace(/,/g, ' ').split(/\s+/).filter(Boolean);
  const slash = tokens.indexOf('/');
  if (slash < 0) return { parts: tokens, alpha: null };
  if (tokens.includes('/', slash + 1)) return null;
  const alpha = tokens[slash + 1];
  if (!alpha || tokens.length !== slash + 2) return null;
  return { parts: tokens.slice(0, slash), alpha };
}

interface Num {
  n: number;
  percent: boolean;
  /** CSS `none`: a missing component, read as zero and not as a unit. */
  none: boolean;
}

function parseNum(token: string): Num | null {
  if (token.toLowerCase() === 'none') return { n: 0, percent: false, none: true };
  const percent = token.endsWith('%');
  const raw = percent ? token.slice(0, -1) : token;
  if (!/^[-+]?(?:\d+\.?\d*|\.\d+)$/.test(raw)) return null;
  const n = Number(raw);
  if (!Number.isFinite(n)) return null;
  return { n, percent, none: false };
}

function alphaOf(token: string | null): number | null {
  if (token === null) return 1;
  const parsed = parseNum(token);
  if (!parsed) return null;
  const value = parsed.percent ? parsed.n / 100 : parsed.n;
  return clamp(value, 0, 1);
}

/** A percent, or a unitless number on the same 0–100 scale, as 0–1. */
function percentUnit(token: string): number | null {
  const parsed = parseNum(token);
  if (!parsed) return null;
  return clamp(parsed.n / 100, 0, 1);
}

function parseRgb(parts: string[], alphaToken: string | null): Rgba | null {
  let channels = parts;
  if (parts.length === 4) {
    if (alphaToken !== null) return null;
    alphaToken = parts[3];
    channels = parts.slice(0, 3);
  }
  if (channels.length !== 3) return null;
  const nums = channels.map(parseNum);
  if (nums.some((num) => num === null)) return null;
  const values = nums as Num[];
  const specified = values.filter((num) => !num.none);
  const percent = specified.some((num) => num.percent);
  if (specified.some((num) => num.percent !== percent)) return null;
  const a = alphaOf(alphaToken);
  if (a === null) return null;
  const [r, g, b] = values.map((num) => byte(percent ? (num.n / 100) * 255 : num.n));
  return { r, g, b, a };
}

function parseHsl(parts: string[], alphaToken: string | null): Rgba | null {
  let channels = parts;
  if (parts.length === 4) {
    if (alphaToken !== null) return null;
    alphaToken = parts[3];
    channels = parts.slice(0, 3);
  }
  if (channels.length !== 3) return null;
  const h = angle(channels[0]);
  const s = percentUnit(channels[1]);
  const l = percentUnit(channels[2]);
  const a = alphaOf(alphaToken);
  if (h === null || s === null || l === null || a === null) return null;
  const [r, g, b] = hslToRgb(h, s, l).map((channel) => byte(channel * 255));
  return { r, g, b, a };
}

function parseHwb(parts: string[], alphaToken: string | null): Rgba | null {
  if (parts.length !== 3) return null;
  const h = angle(parts[0]);
  const w = percentUnit(parts[1]);
  const b = percentUnit(parts[2]);
  const a = alphaOf(alphaToken);
  if (h === null || w === null || b === null || a === null) return null;
  if (w + b >= 1) {
    const gray = byte((w / (w + b)) * 255);
    return { r: gray, g: gray, b: gray, a };
  }
  const [r, g, bl] = hslToRgb(h, 1, 0.5);
  const mix = (channel: number) => byte((channel * (1 - w - b) + w) * 255);
  return { r: mix(r), g: mix(g), b: mix(bl), a };
}

function parseOklch(parts: string[], alphaToken: string | null): Rgba | null {
  if (parts.length !== 3) return null;
  const l = oklchL(parts[0]);
  const c = oklchC(parts[1]);
  const h = angle(parts[2]);
  const a = alphaOf(alphaToken);
  if (l === null || c === null || h === null || a === null) return null;
  return oklchToRgba(l, c, h, a);
}

function parseOklab(parts: string[], alphaToken: string | null): Rgba | null {
  if (parts.length !== 3) return null;
  const l = oklchL(parts[0]);
  const aLab = labAB(parts[1]);
  const bLab = labAB(parts[2]);
  const alpha = alphaOf(alphaToken);
  if (l === null || aLab === null || bLab === null || alpha === null) return null;
  const [r, g, b] = oklabToSrgb(l, aLab, bLab);
  if (!inGamut(oklabToLinear(l, aLab, bLab))) {
    const c = Math.hypot(aLab, bLab);
    const h = c < 1e-6 ? 0 : (Math.atan2(bLab, aLab) * 180) / Math.PI;
    return oklchToRgba(l, c, h < 0 ? h + 360 : h, alpha);
  }
  return { r: byte(r * 255), g: byte(g * 255), b: byte(b * 255), a: alpha };
}

/** Lightness: a 0–1 number, or a percent of that. */
function oklchL(token: string): number | null {
  const parsed = parseNum(token);
  if (!parsed) return null;
  return clamp(parsed.percent ? parsed.n / 100 : parsed.n, 0, 1);
}

/** Chroma: a number, or a percent where 100% is 0.4, as CSS defines it. */
function oklchC(token: string): number | null {
  const parsed = parseNum(token);
  if (!parsed) return null;
  return Math.max(0, parsed.percent ? (parsed.n / 100) * 0.4 : parsed.n);
}

/** OKLab a or b: a number, or a percent where 100% is 0.4. */
function labAB(token: string): number | null {
  const parsed = parseNum(token);
  if (!parsed) return null;
  return parsed.percent ? (parsed.n / 100) * 0.4 : parsed.n;
}

function angle(token: string): number | null {
  if (token.toLowerCase() === 'none') return 0;
  const match = /^([-+]?(?:\d+\.?\d*|\.\d+))(deg|turn|rad|grad)?$/i.exec(token);
  if (!match) return null;
  const n = Number(match[1]);
  const unit = (match[2] ?? 'deg').toLowerCase();
  const degrees =
    unit === 'turn'
      ? n * 360
      : unit === 'rad'
        ? (n * 180) / Math.PI
        : unit === 'grad'
          ? n * 0.9
          : n;
  return ((degrees % 360) + 360) % 360;
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = h / 60;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  let r = 0;
  let g = 0;
  let b = 0;
  if (hp < 1) [r, g, b] = [c, x, 0];
  else if (hp < 2) [r, g, b] = [x, c, 0];
  else if (hp < 3) [r, g, b] = [0, c, x];
  else if (hp < 4) [r, g, b] = [0, x, c];
  else if (hp < 5) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  const m = l - c / 2;
  return [r + m, g + m, b + m];
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return [0, 0, l];
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = 0;
  if (max === rn) h = (gn - bn) / d + (gn < bn ? 6 : 0);
  else if (max === gn) h = (bn - rn) / d + 2;
  else h = (rn - gn) / d + 4;
  return [h * 60, s, l];
}

// OKLab, as CSS Color uses it (Björn Ottosson's updated matrices).

function linearToOklab(r: number, g: number, b: number): [number, number, number] {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

function oklabToLinear(l: number, a: number, b: number): [number, number, number] {
  const lp = l + 0.3963377774 * a + 0.2158037573 * b;
  const mp = l - 0.1055613458 * a - 0.0638541728 * b;
  const sp = l - 0.0894841775 * a - 1.291485548 * b;
  const l3 = lp * lp * lp;
  const m3 = mp * mp * mp;
  const s3 = sp * sp * sp;
  return [
    4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3,
    -1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3,
    -0.0041960863 * l3 - 0.7034186147 * m3 + 1.707614701 * s3,
  ];
}

function oklabToSrgb(l: number, a: number, b: number): [number, number, number] {
  return oklabToLinear(l, a, b).map(linearToSrgb) as [number, number, number];
}

function rgbToOklch(rgba: Rgba): { l: number; c: number; h: number } {
  const [l, a, b] = linearToOklab(
    srgbToLinear(rgba.r / 255),
    srgbToLinear(rgba.g / 255),
    srgbToLinear(rgba.b / 255)
  );
  const c = Math.hypot(a, b);
  if (c < 1e-6) return { l, c: 0, h: 0 };
  let h = (Math.atan2(b, a) * 180) / Math.PI;
  if (h < 0) h += 360;
  return { l, c, h };
}

function oklchToLinear(l: number, c: number, h: number): [number, number, number] {
  const rad = (h * Math.PI) / 180;
  return oklabToLinear(l, c * Math.cos(rad), c * Math.sin(rad));
}

/** Same hue and lightness, chroma lowered until the color fits in sRGB. */
function oklchToRgba(l: number, c: number, h: number, a: number): Rgba {
  let chroma = Math.max(0, c);
  if (!inGamut(oklchToLinear(l, chroma, h))) {
    let lo = 0;
    let hi = chroma;
    for (let i = 0; i < 24; i++) {
      const mid = (lo + hi) / 2;
      if (inGamut(oklchToLinear(l, mid, h))) lo = mid;
      else hi = mid;
    }
    chroma = lo;
  }
  const [r, g, b] = oklchToLinear(l, chroma, h).map(linearToSrgb);
  return { r: byte(r * 255), g: byte(g * 255), b: byte(b * 255), a };
}

function inGamut(rgb: [number, number, number]): boolean {
  return rgb.every((channel) => channel >= -1e-4 && channel <= 1 + 1e-4);
}

function shadesFor(rgba: Rgba): { shades: Rgba[]; index: number } {
  const { l, c, h } = rgbToOklch(rgba);
  let index = 0;
  let best = Infinity;
  SHADE_LIGHTNESS.forEach((step, at) => {
    const distance = Math.abs(step - l);
    if (distance < best) {
      best = distance;
      index = at;
    }
  });
  const shades = SHADE_LIGHTNESS.map((step, at) =>
    at === index ? { ...rgba } : oklchToRgba(step, c, h, rgba.a)
  );
  return { shades, index };
}

function formatHex(rgba: Rgba): string {
  const body = hexByte(rgba.r) + hexByte(rgba.g) + hexByte(rgba.b);
  if (rgba.a >= 0.999) return `#${body}`;
  return `#${body}${hexByte(rgba.a * 255)}`;
}

function formatRgb(rgba: Rgba): string {
  return `rgb(${rgba.r} ${rgba.g} ${rgba.b}${alphaSuffix(rgba.a)})`;
}

function formatHsl(rgba: Rgba): string {
  const [h, s, l] = rgbToHsl(rgba.r, rgba.g, rgba.b);
  return `hsl(${trim(h, 1)} ${Math.round(s * 100)}% ${Math.round(l * 100)}%${alphaSuffix(rgba.a)})`;
}

function formatOklch(rgba: Rgba): string {
  const { l, c, h } = rgbToOklch(rgba);
  return `oklch(${trim(l, 3)} ${trim(c, 3)} ${trim(h, 2)}${alphaSuffix(rgba.a)})`;
}

function alphaSuffix(a: number): string {
  if (a >= 0.999) return '';
  return ` / ${trim(a, 3)}`;
}

function contrast(fg: Rgba, bg: Rgba): number {
  const over = composite(fg, bg);
  const lighter = Math.max(luminance(over), luminance(bg));
  const darker = Math.min(luminance(over), luminance(bg));
  return (lighter + 0.05) / (darker + 0.05);
}

function composite(fg: Rgba, bg: Rgba): Rgba {
  const a = fg.a + bg.a * (1 - fg.a);
  if (a === 0) return { r: 0, g: 0, b: 0, a: 0 };
  const channel = (front: number, back: number) =>
    Math.round((front * fg.a + back * bg.a * (1 - fg.a)) / a);
  return { r: channel(fg.r, bg.r), g: channel(fg.g, bg.g), b: channel(fg.b, bg.b), a: 1 };
}

function luminance(rgba: Rgba): number {
  return 0.2126 * linearOf(rgba.r) + 0.7152 * linearOf(rgba.g) + 0.0722 * linearOf(rgba.b);
}

function linearOf(channel: number): number {
  const s = channel / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function srgbToLinear(c: number): number {
  const s = clamp(c, 0, 1);
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb(c: number): number {
  const x = clamp(c, 0, 1);
  return x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055;
}

function byte(channel: number): number {
  return Math.min(255, Math.max(0, Math.round(channel)));
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, n));
}

function hexByte(n: number): string {
  return byte(n).toString(16).padStart(2, '0');
}

function trim(n: number, digits: number): string {
  return n.toFixed(digits).replace(/\.?0+$/, '');
}
