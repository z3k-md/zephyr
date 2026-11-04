// Downloads Unicode's emoji list and the English CLDR names, and writes the compact
// catalog the bar searches. Fully-qualified emoji only, through Unicode 15.1, which current
// Windows 11 Segoe UI Emoji and macOS both draw. Skin-tone variants are not their own rows;
// each base records the code-point indexes a tone is inserted after, taken from Unicode's
// first single-tone form (the same tone on every person, so people holding hands is
// 🧑🏻‍🤝‍🧑🏻 and not a mixed pair). Variation selectors are ignored while matching a toned
// variant to its base, because a tone drops U+FE0F on the part it colors (270C 1F3FB
// belongs with 270C FE0F) and leaves it on every other part. The subgroup is recorded so
// country flags can be hidden where the font can't draw them. Run with `node scripts/emoji.mjs`
// when the catalog should move forward.

import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const VERSION = [15, 1];
const EMOJI_TEST = 'https://unicode.org/Public/emoji/15.1/emoji-test.txt';
const CLDR = 'https://raw.githubusercontent.com/unicode-org/cldr/release-44/common';
const SKIN = new Set([0x1f3fb, 0x1f3fc, 0x1f3fd, 0x1f3fe, 0x1f3ff]);
const VS = 0xfe0f;
const HANDSHAKE = 0x1f91d;
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

function pointsOf(text) {
  const points = [];
  for (const char of text) {
    const point = char.codePointAt(0);
    if (point !== undefined) points.push(point);
  }
  return points;
}

function keyOf(points) {
  return points.map((point) => point.toString(16)).join(' ');
}

/** Skin modifiers and variation selectors removed, so a toned form finds its base. */
function bareKey(points) {
  return keyOf(points.filter((point) => point !== VS && !SKIN.has(point)));
}

function newer([major, minor], [capMajor, capMinor]) {
  return major > capMajor || (major === capMajor && minor > capMinor);
}

function singleTone(points) {
  const mods = points.filter((point) => SKIN.has(point));
  return mods.length > 0 && mods.every((mod) => mod === mods[0]);
}

/**
 * Indexes in `base` that take a modifier. A variation selector is dropped only
 * when it sits on a toned part; one on any other part stays in the base.
 * Null when the two sequences aren't the same emoji.
 */
function tonePositions(base, toned) {
  const positions = [];
  let i = 0;
  let j = 0;
  while (i < base.length && j < toned.length) {
    if (base[i] !== toned[j]) return null;
    if (j + 1 < toned.length && SKIN.has(toned[j + 1])) {
      positions.push(i);
      j += 2;
      i += 1;
      if (i < base.length && base[i] === VS && (j >= toned.length || toned[j] !== VS)) i += 1;
    } else {
      i += 1;
      j += 1;
    }
  }
  if (i !== base.length || j !== toned.length || positions.length === 0) return null;
  return positions;
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
const subgroups = [];
const subgroupIndex = new Map();
let group = '';
let subgroup = '';
/** First single-tone sequence for a base, keyed with variation selectors removed. */
const toned = new Map();
const emoji = [];

for (const raw of test.split(/\r?\n/)) {
  const heading = /^# group: (.+)$/.exec(raw);
  if (heading) {
    group = heading[1].trim();
    continue;
  }
  const sub = /^# subgroup: (.+)$/.exec(raw);
  if (sub) {
    subgroup = sub[1].trim();
    continue;
  }
  const parsed = line.exec(raw);
  if (!parsed) continue;
  if (group === 'Component') continue;
  const version = [Number(parsed[2]), Number(parsed[3])];
  if (newer(version, VERSION)) continue;
  const points = codePoints(parsed[1]);
  if (points.some((point) => SKIN.has(point))) {
    if (!singleTone(points)) continue;
    const key = bareKey(points);
    if (!toned.has(key)) toned.set(key, points);
    continue;
  }

  if (!groupIndex.has(group)) {
    groupIndex.set(group, groups.length);
    groups.push(group);
  }
  if (!subgroupIndex.has(subgroup)) {
    subgroupIndex.set(subgroup, subgroups.length);
    subgroups.push(subgroup);
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
    u: subgroupIndex.get(subgroup),
    s: 0,
    k: keywords,
  });
}

const bareSeen = new Map();
for (const entry of emoji) {
  const points = pointsOf(entry.c);
  const key = bareKey(points);
  const other = bareSeen.get(key);
  if (other) throw new Error(`variation selectors don't tell ${other} from ${entry.n}`);
  bareSeen.set(key, entry.n);
  const sample = toned.get(key);
  if (!sample) continue;
  const positions = tonePositions(points, sample);
  if (!positions) throw new Error(`couldn't read a skin tone for ${entry.n} (${keyOf(points)})`);
  entry.s = 1;
  entry.p = positions;
}

if (emoji.length < 1000) {
  throw new Error(`only ${emoji.length} emoji parsed; the catalog looks wrong`);
}
for (const name of ['country-flag', 'subdivision-flag', 'flag']) {
  if (!subgroups.includes(name)) throw new Error(`missing the ${name} subgroup`);
}
for (const name of ['victory hand', 'writing hand', 'index pointing up']) {
  const entry = emoji.find((item) => item.n === name);
  if (!entry?.p?.length) throw new Error(`${name} should take a skin tone`);
}
const holding = emoji.find((item) => item.n === 'people holding hands');
const holdingPoints = holding ? pointsOf(holding.c) : [];
if (
  !holding?.p ||
  holding.p.length < 2 ||
  holding.p.some((index) => holdingPoints[index] === HANDSHAKE)
) {
  throw new Error('people holding hands should tone both people and not the handshake');
}

const rows = emoji.map((entry) => {
  const row = {
    c: entry.c,
    n: entry.n,
    g: entry.g,
    u: entry.u,
    s: entry.s,
  };
  if (entry.p) row.p = entry.p;
  row.k = entry.k;
  return row;
});

const body = {
  version: VERSION.join('.'),
  groups,
  subgroups,
  emoji: rows,
};
const json =
  '{\n' +
  `  "version": ${JSON.stringify(body.version)},\n` +
  `  "groups": ${JSON.stringify(body.groups)},\n` +
  `  "subgroups": ${JSON.stringify(body.subgroups)},\n` +
  '  "emoji": [\n' +
  body.emoji.map((entry) => `    ${JSON.stringify(entry)}`).join(',\n') +
  '\n  ]\n}\n';
writeFileSync(OUT, json);
const tonedCount = emoji.filter((entry) => entry.s).length;
console.log(
  `wrote ${emoji.length} emoji (${tonedCount} take a skin tone) in ${groups.length} groups to ${OUT}`
);
