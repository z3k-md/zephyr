// Reads Zephyr's logs (the installed app's and the dev build's) and reports the warnings,
// errors, panics and unclean exits written since the last scan, grouped so one bug repeated
// a hundred times reads as one line.
//
//   node scripts/logscan.mjs              what's new since the last scan, then marks it read
//   node scripts/logscan.mjs --peek       the same, without marking it read
//   node scripts/logscan.mjs --since 2h   everything in the last 2 hours (m, h or d)
//   node scripts/logscan.mjs --all        everything the logs still hold
//   node scripts/logscan.mjs --hook       quiet unless there's something new (for agent hooks)
//
// What was read last is kept in logs/.logscan.json, which git ignores.

import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const STATE_FILE = join(ROOT, 'logs', '.logscan.json');
const LINE =
  /^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{3}) (ERROR|WARN|INFO|DEBUG|TRACE) \[([^\]]*)\] (.*)$/;
const SAMPLE_LINES = 6;

/** The folders logger.rs writes to: the installed app's, and dev builds' beside it. */
export function logDirs(env = process.env, platform = process.platform) {
  const base =
    platform === 'darwin'
      ? join(env.HOME ?? homedir(), 'Library', 'Logs', 'Zephyr')
      : join(env.LOCALAPPDATA ?? env.HOME ?? homedir(), 'Zephyr', 'logs');
  return { release: base, dev: join(base, 'dev') };
}

/** Log lines as entries; a line without a timestamp (a backtrace, a stack) joins the last one. */
export function parseLog(text, source) {
  const entries = [];
  for (const line of text.split(/\r?\n/)) {
    const match = LINE.exec(line);
    if (match) {
      const [, time, level, target, message] = match;
      entries.push({ source, time, level, target, message });
    } else if (entries.length && line.trim()) {
      entries[entries.length - 1].message += `\n${line}`;
    }
  }
  return entries;
}

/** What makes two entries the same issue: numbers, ids and addresses don't count. */
export function signature(entry) {
  const lines = entry.message.split('\n');
  // A panic's first line is where; the second is what.
  const head = entry.message.startsWith('panic') ? lines.slice(0, 2).join(' ') : lines[0];
  const normalized = head
    .replace(/^panic on thread [^:]+:/, 'panic:')
    .replace(/0x[0-9a-f]+/gi, '0x…')
    .replace(/\b[0-9a-f]{8}-[0-9a-f-]{27,}\b/gi, '<id>')
    .replace(/\d+(\.\d+)?/g, '#');
  return `${entry.level} [${entry.target}] ${normalized}`;
}

const STARTED = /^Zephyr \S+ \((dev|release)\) starting on /;
const EXITING = /^Zephyr exiting/;

/**
 * Each run that started again without having logged its exit: a crash, a kill, an update's
 * installer or the computer shutting down. Only runs logged since logger.rs began marking
 * both ends count, and only the installed app's: `tauri dev` kills the dev build at every
 * rebuild.
 */
export function uncleanExits(entries) {
  const found = [];
  let open = null;
  for (const entry of entries) {
    if (STARTED.test(entry.message)) {
      // Dated when the next run starts, which is when it's known: a scan made while the
      // crashed run was still up has already passed its start.
      if (open) {
        const message = `the run started ${open.time} ended without a clean exit`;
        found.push({ ...open, time: entry.time, level: 'WARN', message });
      }
      open = entry;
    } else if (EXITING.test(entry.message)) {
      open = null;
    }
  }
  return found;
}

/** One source's warnings, errors and unclean exits after `from`, a log timestamp. */
export function flagged(entries, from = '') {
  const lines = entries.filter(
    (entry) => entry.time > from && (entry.level === 'ERROR' || entry.level === 'WARN')
  );
  const exits = entries.some((entry) => entry.source === 'dev') ? [] : uncleanExits(entries);
  return [...lines, ...exits.filter((entry) => entry.time > from)];
}

/** Flagged entries grouped by signature, panics first, then errors, most frequent first. */
export function issues(entries) {
  const groups = new Map();
  for (const entry of entries) {
    const key = `${entry.source} ${signature(entry)}`;
    const group = groups.get(key);
    if (group) {
      group.count += 1;
      if (entry.time < group.first) group.first = entry.time;
      if (entry.time > group.last) {
        group.last = entry.time;
        group.sample = entry;
      }
    } else {
      groups.set(key, { key, count: 1, first: entry.time, last: entry.time, sample: entry });
    }
  }
  const rank = (group) =>
    group.sample.message.startsWith('panic') ? 2 : group.sample.level === 'ERROR' ? 1 : 0;
  return [...groups.values()].sort((a, b) => rank(b) - rank(a) || b.count - a.count);
}

export function report(groups) {
  return groups
    .map((group) => {
      const { sample } = group;
      const when = group.count > 1 ? `${group.count}× ${group.first} → ${group.last}` : sample.time;
      const lines = sample.message.split('\n');
      const body = lines.slice(0, SAMPLE_LINES).map((line) => `    ${line}`);
      if (lines.length > SAMPLE_LINES) body.push(`    … ${lines.length - SAMPLE_LINES} more lines`);
      return [`${sample.level} [${sample.target}] (${sample.source}) ${when}`, ...body].join('\n');
    })
    .join('\n\n');
}

/** `2h` as the log's own timestamp format, that long before `now`. */
export function sinceStamp(duration, now = new Date()) {
  const match = /^(\d+)([mhd])$/.exec(duration ?? '');
  if (!match) throw new Error(`--since wants a duration like 30m, 2h or 1d, not ${duration}`);
  const unit = { m: 60_000, h: 3_600_000, d: 86_400_000 }[match[2]];
  return stamp(new Date(now.getTime() - Number(match[1]) * unit));
}

/** A local time as logger.rs writes it, so the two compare as strings. */
export function stamp(date) {
  const pad = (n, width = 2) => String(n).padStart(width, '0');
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`
  );
}

function readSource(source, dir) {
  if (!existsSync(dir)) return [];
  const files = readdirSync(dir)
    .filter((name) => /^zephyr_r(\d+|CURRENT)\.log$/.test(name))
    .sort((a, b) => (a.includes('CURRENT') ? 1 : b.includes('CURRENT') ? -1 : a.localeCompare(b)));
  return files.flatMap((name) => {
    try {
      return parseLog(readFileSync(join(dir, name), 'utf8'), source);
    } catch {
      // Rotated away between listing and reading: its lines are in the next file.
      return [];
    }
  });
}

function readState() {
  try {
    return JSON.parse(readFileSync(STATE_FILE, 'utf8'));
  } catch {
    return {};
  }
}

function main(args) {
  const flag = (name) => args.includes(name);
  const hook = flag('--hook');
  const sinceAt = args.indexOf('--since');
  const state = readState();
  const marking = !flag('--peek') && !flag('--all') && sinceAt < 0;

  const found = [];
  const next = { ...state };
  for (const [source, dir] of Object.entries(logDirs())) {
    const entries = readSource(source, dir);
    const from = flag('--all')
      ? ''
      : sinceAt >= 0
        ? sinceStamp(args[sinceAt + 1])
        : (state[source] ?? '');
    found.push(...flagged(entries, from));
    if (entries.length) next[source] = entries[entries.length - 1].time;
  }

  const groups = issues(found);
  if (groups.length) {
    const total = groups.reduce((sum, group) => sum + group.count, 0);
    const scope = flag('--all')
      ? 'in the logs'
      : sinceAt >= 0
        ? `in the last ${args[sinceAt + 1]}`
        : 'since the last scan';
    console.log(`Zephyr logs: ${groups.length} issues (${total} lines) ${scope}\n`);
    console.log(report(groups));
    if (hook)
      console.log(
        '\n(Investigate before carrying on if any of this looks like a bug. Read the full log with node scripts/logscan.mjs --since 1h.)'
      );
  } else if (!hook) {
    console.log('Zephyr logs: nothing new.');
  }

  if (marking) {
    mkdirSync(dirname(STATE_FILE), { recursive: true });
    writeFileSync(STATE_FILE, `${JSON.stringify(next, null, 2)}\n`);
  }
}

if (process.argv[1]?.replaceAll('\\', '/').endsWith('scripts/logscan.mjs')) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    // A hook must never block a prompt.
    console.error(String(error?.message ?? error));
    process.exitCode = process.argv.includes('--hook') ? 0 : 1;
  }
}
