// Downloads Unicode's emoji list and the English CLDR names, and writes the compact
// catalog the bar searches. Fully-qualified emoji only, through Unicode 15.1, which current
// Windows 11 Segoe UI Emoji and macOS both draw. Skin-tone variants are not their own rows;
// the base is flagged instead. Run with `node scripts/emoji.mjs` when the catalog should
// move forward.

import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const VERSION = [15, 1];
const EMOJI_TEST = 'https://unicode.org/Public/emoji/15.1/emoji-test.txt';
const CLDR = 'https://raw.githubusercontent.com/unicode-org/cldr/release-44/common';
const SKIN = new Set([0x1f3fb, 0x1f3fc, 0x1f3fd, 0x1f3fe, 0x1f3ff]);
const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'bar', 'emoji', 'data.json');

const line = /^([0-9A-Fa-f][0-9A-Fa-f ]*?)\s+;\s+fully-qualified\s+#\s+\S+\s+E(\d+)\.(\d+)\s+(.+)$/;

async function text(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url} returned ${response.status}`);
  return response.text();
}

function decode(value) {
  return value
    .replaceAll('&amp;', '&')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&quot;', '"')
    .replaceAll('&apos;', "'");
}

/** CLDR annotations: keywords, and the spoken name, keyed by the emoji. */
function annotations(xml) {
  const keywords = new Map();
  const names = new Map();
  const pattern = /<annotation cp="([^"]+)"(?:\s+type="([^"]+)")?\s*>([^<]*)<\/annotation>/g;
  for (const match of xml.matchAll(pattern)) {
    const chars = decode(match[1]);
    const body = decode(match[3]).trim();
    if (!body) continue;
    const keys = [chars, chars.replaceAll('\uFE0F', '')];
    if (match[2] === 'tts') {
      for (const key of keys) names.set(key, body);
    } else if (!match[2]) {
      const words = body
        .split('|')
        .map((word) => word.trim().toLowerCase())
        .filter(Boolean);
      for (const key of keys) keywords.set(key, words);
    }
  }
  return { keywords, names };
}

function codePoints(hexes) {
  return hexes
    .trim()
    .split(/\s+/)
    .map((hex) => Number.parseInt(hex, 16));
}

function keyOf(points) {
  return points.map((point) => point.toString(16)).join(' ');
}

function newer([major, minor], [capMajor, capMinor]) {
  return major > capMajor || (major === capMajor && minor > capMinor);
}

const [test, annotationsXml, derivedXml] = await Promise.all([
  text(EMOJI_TEST),
  text(`${CLDR}/annotations/en.xml`),
  text(`${CLDR}/annotationsDerived/en.xml`),
]);

const baseNotes = annotations(annotationsXml);
const derivedNotes = annotations(derivedXml);
const notes = {
  keywords: new Map([...baseNotes.keywords, ...derivedNotes.keywords]),
  names: new Map([...baseNotes.names, ...derivedNotes.names]),
};

const groups = [];
const groupIndex = new Map();
let group = '';
/** Hex sequences of bases that have a fully-qualified skin-tone variant. */
const toned = new Set();
/** Code points a skin tone is inserted after. */
const bases = new Set();
const emoji = [];

for (const raw of test.split(/\r?\n/)) {
  const heading = /^# group: (.+)$/.exec(raw);
  if (heading) {
    group = heading[1].trim();
    continue;
  }
  const parsed = line.exec(raw);
  if (!parsed) continue;
  if (group === 'Component') continue;
  const version = [Number(parsed[2]), Number(parsed[3])];
  if (newer(version, VERSION)) continue;
  const points = codePoints(parsed[1]);
  if (points.some((point) => SKIN.has(point))) {
    const base = points.filter((point) => !SKIN.has(point));
    toned.add(keyOf(base));
    for (let i = 0; i < points.length; i++) {
      if (!SKIN.has(points[i]) || i === 0) continue;
      let previous = i - 1;
      while (previous > 0 && points[previous] === 0xfe0f) previous--;
      bases.add(points[previous]);
    }
    continue;
  }

  if (!groupIndex.has(group)) {
    groupIndex.set(group, groups.length);
    groups.push(group);
  }
  const chars = String.fromCodePoint(...points);
  const name = (notes.names.get(chars) ?? parsed[4]).trim().toLowerCase();
  const words = notes.keywords.get(chars) ?? [];
  const seen = new Set([name]);
  const keywords = [];
  for (const word of words) {
    if (seen.has(word) || word.length > 40) continue;
    seen.add(word);
    keywords.push(word);
    if (keywords.length === 16) break;
  }
  emoji.push({
    c: chars,
    n: name,
    g: groupIndex.get(group),
    s: 0,
    k: keywords,
  });
}

for (const entry of emoji) {
  const points = [...entry.c].map((char) => char.codePointAt(0));
  if (toned.has(keyOf(points))) entry.s = 1;
}

if (emoji.length < 1000) {
  throw new Error(`only ${emoji.length} emoji parsed; the catalog looks wrong`);
}

const body = {
  version: VERSION.join('.'),
  groups,
  bases: [...bases].sort((a, b) => a - b),
  emoji,
};
const json =
  '{\n' +
  `  "version": ${JSON.stringify(body.version)},\n` +
  `  "groups": ${JSON.stringify(body.groups)},\n` +
  `  "bases": ${JSON.stringify(body.bases)},\n` +
  '  "emoji": [\n' +
  body.emoji.map((entry) => `    ${JSON.stringify(entry)}`).join(',\n') +
  '\n  ]\n}\n';
writeFileSync(OUT, json);
const tonedCount = emoji.filter((entry) => entry.s).length;
console.log(
  `wrote ${emoji.length} emoji (${tonedCount} take a skin tone) in ${groups.length} groups to ${OUT}`
);
