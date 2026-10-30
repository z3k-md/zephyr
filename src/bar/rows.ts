// Turns a suggest response into the bar's sectioned list: the query row, command rows and
// section headers. Pure functions, so the bar only wires them up.

import { shortcutKeys } from '../keys';
import type { Binding, LaunchGroup, Snapshot, SuggestionRow, SuggestResponse } from '../types';
import type { PastAnswer } from './answerHistory';

export interface RowCtx {
  snapshot: Snapshot | null;
  /** The parked workspace ("Back to …"), if any. */
  parked: string;
  parkedName: string;
  /** The text as typed, for matching command names; empty in a scoped search. */
  typed: string;
  /** Answers copied before, newest first, shown under a new calculation. */
  pastAnswers?: PastAnswer[];
}

export interface Group {
  title: string;
  start: number;
  rows: SuggestionRow[];
}

const SECTION_TITLES: Record<string, string> = {
  apps: 'Apps',
  answer: 'Answers',
  recent: 'Recent',
  files: 'Files',
  settings: 'Settings',
  notes: 'Notes',
  views: 'Utilities',
  commands: 'Commands',
  shell: 'Shell',
  power: 'Power',
  bookmarks: 'Bookmarks',
  windows: 'Windows',
  ports: 'Ports',
};

export function rowKey(row: SuggestionRow): string {
  if (row.kind === 'query') return 'query';
  if (row.kind === 'netCard') return `netCard-${row.target}`;
  if (row.portUse) {
    const use = row.portUse;
    return `${row.kind}-${use.protocol}-${use.port}-${use.pid}-${use.state}`;
  }
  // A window or an app id tells apart rows that share a label (two windows titled alike).
  const identity = row.window ?? row.appId ?? row.path ?? row.noteId ?? row.label;
  return `${row.kind}-${row.target}-${identity}`;
}

// What the bar's parts are called: answers are cards inline in the list, views take over
// the bar, commands are one-shot actions, and the status bar is the footer and the chips
// in the search row. Utilities is the umbrella name for the features Zephyr ships.
const VIEWS = ['back', 'clip', 'notes', 'resources', 'network', 'shell', 'typing'];

/** What a binding's target is, as Settings › Shortcuts names it. */
export function targetKind(target: string): string {
  if (target.startsWith('app:')) return 'App';
  if (target.startsWith('bookmark:')) return 'Bookmark';
  if (target.startsWith('setting:')) return 'Settings page';
  if (target.startsWith('scope:')) return 'Search';
  if (target.startsWith('view:') && VIEWS.includes(target.slice(5))) return 'View';
  return 'Command';
}

function view(
  id: string,
  label: string,
  icon: string,
  extra: Partial<SuggestionRow>
): SuggestionRow {
  return {
    label,
    hint: '',
    query: '',
    target: id,
    kind: 'view',
    section: VIEWS.includes(id) ? 'views' : 'commands',
    icon,
    ...extra,
  };
}

/** How many views the empty bar offers, filled by what's opened most. */
const VIEW_SLOTS = 3;
/** The views it can offer, in the order a fresh install shows them. */
const OFFERED_VIEWS = ['clip', 'notes', 'network', 'resources', 'shell', 'typing'];

/** The views to offer: most opened lately first (uses, fading over days, as apps rank),
 *  then the fresh-install order. */
function offeredViews(ctx: RowCtx): string[] {
  const settings = ctx.snapshot;
  const opens = settings?.viewOpens ?? [];
  const now = Date.now() / 1000;
  const score = (id: string) => {
    const entry = opens.find((open) => open.appId === id);
    if (!entry) return 0;
    const ageDays = Math.max(0, now - entry.lastUsed) / 86400;
    return entry.uses / (1 + ageDays);
  };
  return OFFERED_VIEWS.filter((id) => id !== 'clip' || settings?.clipboard.enabled)
    .map((id, order) => ({ id, order, score: score(id) }))
    .sort((a, b) => b.score - a.score || a.order - b.order)
    .slice(0, VIEW_SLOTS)
    .map((entry) => entry.id);
}

/** Rows on the empty bar that open a view (or type in a question with an answer). */
function jumpRows(ctx: RowCtx): SuggestionRow[] {
  const rows: SuggestionRow[] = [];
  const settings = ctx.snapshot;
  if (ctx.parked && ctx.parkedName) {
    rows.push(
      view('back', `Back to ${ctx.parkedName}`, 'glyph:back', {
        hint: 'Esc again to clear',
        keyHints: [{ keys: 'esc', label: 'again to clear' }],
      })
    );
  }
  for (const id of offeredViews(ctx)) {
    const command = COMMANDS.find((item) => item.id === id);
    if (!command) continue;
    const shortcut =
      id === 'clip' ? settings?.clipboard.shortcut : id === 'notes' ? settings?.notesShortcut : '';
    const keys = shortcut ? shortcutKeys(shortcut) : [];
    rows.push(view(id, command.label, command.icon, keys.length ? { keys } : {}));
  }
  // Types "ip" in, so the addresses answer below it.
  rows.push(view('ip', 'IP address', 'glyph:network', {}));
  return rows;
}

interface Command {
  id: string;
  label: string;
  icon: string;
  /** Names it answers to besides its label, lowercase. */
  names: string[];
  /** Words that offer it without letting Enter take it ("cpu" may be a search). */
  keywords?: string[];
}

// Every mode and scoped search, by name, so none needs a bang: typing "files" offers the
// files search, "typing" the typing test.
const COMMANDS: Command[] = [
  {
    id: 'clip',
    label: 'Clipboard history',
    icon: 'glyph:clipboard',
    names: ['clipboard', 'clip', 'paste history'],
  },
  { id: 'notes', label: 'Notes', icon: 'glyph:note', names: ['note'] },
  {
    id: 'resources',
    label: 'System Resources',
    icon: 'glyph:cpu',
    names: ['resources', 'top', 'htop', 'btop', 'activity monitor'],
    keywords: [
      'cpu',
      'memory',
      'ram',
      'gpu',
      'processes',
      'performance',
      'battery',
      'disk',
      'activity',
      'task manager',
      'kill',
      'force quit',
    ],
  },
  {
    id: 'network',
    label: 'Network',
    icon: 'glyph:pulse',
    names: ['network', 'internet', 'connection'],
    keywords: ['ping', 'latency', 'jitter', 'lag', 'packet loss'],
  },
  {
    id: 'speedtest',
    label: 'Run speed test',
    icon: 'glyph:gauge',
    names: ['speed test', 'speedtest', 'internet speed', 'test speed'],
  },
  { id: 'shell', label: 'Shell', icon: 'glyph:prompt', names: ['terminal', 'command line'] },
  {
    id: 'typing',
    label: 'Typing test',
    icon: 'glyph:keyboard',
    names: ['typing', 'type test', 'monkeytype', 'wpm'],
  },
  {
    id: 'scope:files',
    label: 'Search files',
    icon: 'glyph:folder',
    names: ['files', 'file', 'find file', 'folders'],
  },
  {
    id: 'scope:bookmarks',
    label: 'Search bookmarks',
    icon: 'glyph:link',
    names: ['bookmarks', 'bookmark'],
  },
  {
    id: 'scope:settings',
    label: 'Search settings',
    icon: 'glyph:gear',
    names: ['settings', 'setting', 'preferences'],
  },
  { id: 'scope:apps', label: 'Search apps', icon: 'glyph:grid', names: ['apps', 'applications'] },
];

/**
 * Commands whose name starts with the typed text (at least two letters), with whether the
 * text is exactly one of its names; only an exact name may take Enter.
 */
function commandRows(ctx: RowCtx): { rows: SuggestionRow[]; exact: number | null } {
  const typed = ctx.typed.trim().toLowerCase().replace(/\s+/g, ' ');
  if (typed.length < 2 || /^[!>]/.test(typed)) return { rows: [], exact: null };
  const settings = ctx.snapshot;
  const rows: SuggestionRow[] = [];
  let exact: number | null = null;
  for (const command of COMMANDS) {
    if (command.id === 'clip' && !settings?.clipboard.enabled) continue;
    const names = [command.label.toLowerCase(), ...command.names];
    const words = command.label.toLowerCase().split(' ');
    const keyword = (command.keywords ?? []).some(
      (word) => word.startsWith(typed) && typed.length >= Math.min(3, word.length)
    );
    if (
      !names.some((name) => name.startsWith(typed)) &&
      !words.some((w) => w.startsWith(typed)) &&
      !keyword
    ) {
      continue;
    }
    // A bare "settings" opens System Settings; the settings search stays one row down.
    if (exact === null && names.includes(typed) && command.id !== 'scope:settings') {
      exact = rows.length;
    }
    let keys: string[] = [];
    if (command.id === 'clip' && settings?.clipboard.shortcut) {
      keys = shortcutKeys(settings.clipboard.shortcut);
    } else if (command.id === 'notes' && settings?.notesShortcut) {
      keys = shortcutKeys(settings.notesShortcut);
    }
    rows.push(view(command.id, command.label, command.icon, keys.length ? { keys } : {}));
  }
  // Launch groups by name; Enter on one opens all of it.
  for (const group of settings?.launchGroups ?? []) {
    const name = group.name.toLowerCase();
    const words = name.split(/\s+/);
    if (!name.startsWith(typed) && !words.some((word) => word.startsWith(typed))) continue;
    if (exact === null && name === typed) exact = rows.length;
    rows.push(groupRow(group));
  }
  return { rows, exact };
}

/** Typed exactly, these show the network card. */
const NET_WORDS = ['network', 'net', 'ping', 'internet', 'connection', 'latency', 'lag', 'jitter'];

/** The network card as the list's first row; `ip` makes it the addresses card. */
function netCard(target: 'network' | 'ip'): SuggestionRow {
  return { kind: 'netCard', target, label: 'Network', query: '', hint: '', section: 'card' };
}

function commandRow(command: Command): SuggestionRow {
  return view(command.id, command.label, command.icon, {});
}

/** A launch group's row: its name, its icon, and its apps' icons small beside it. */
function groupRow(group: LaunchGroup, hint = ''): SuggestionRow {
  const apps = group.items.filter((item) => item.kind === 'app');
  return view(
    `group:${group.id}`,
    group.name,
    group.icon ? `glyph:${group.icon}` : apps[0] ? `app:${apps[0].value}` : 'glyph:grid',
    {
      hint: hint || `${group.items.length} item${group.items.length === 1 ? '' : 's'}`,
      icons: apps.slice(0, 5).map((item) => `app:${item.value}`),
    }
  );
}

/** The rows the bar lists, and which one Enter takes before the user moves. */
export function buildRows(
  response: SuggestResponse,
  ctx: RowCtx
): { rows: SuggestionRow[]; preselect: number | null } {
  if (response.mode === 'recent' && !response.text) {
    // Recent apps and searches lead (the suggester orders them); commands sit below.
    return { rows: [...response.items, ...jumpRows(ctx)], preselect: null };
  }
  const rows: SuggestionRow[] = [...response.items];
  let preselect = response.preselect;
  // Free text going to the web gets its own row at the top, selected. When a settings page
  // leads ("wifi settings"), the web row comes second, under the best page.
  const settingsFirst = response.preselect === 0 && rows[0]?.kind === 'setting';
  if (
    response.mode === 'search' &&
    response.target &&
    (response.preselect === null || settingsFirst) &&
    response.text
  ) {
    const link = response.target === 'url';
    rows.splice(settingsFirst ? 1 : 0, 0, {
      kind: 'query',
      label: response.text,
      query: response.text,
      target: response.target,
      section: 'web',
      icon: link ? 'glyph:link' : 'glyph:google',
      hint: link ? 'Open' : 'Search · AI Mode',
      keyHints: link
        ? undefined
        : [
            { keys: 'enter', label: 'Search' },
            { keys: 'shift+enter', label: 'AI Mode' },
          ],
    });
    preselect = 0;
  }
  // Commands by name sit below the results; one named exactly takes Enter, unless an app
  // has that exact name too ("Terminal" opens Terminal).
  const commands = commandRows(ctx);
  const typed = ctx.typed.trim().toLowerCase();
  const appNamed = rows.some((row) => row.kind === 'app' && row.label.toLowerCase() === typed);
  if (commands.exact !== null && !appNamed) {
    const [named] = commands.rows.splice(commands.exact, 1);
    rows.unshift(named);
    preselect = 0;
  }
  rows.push(...commands.rows);
  // "ping", "network" and the like: the network card on top, taking Enter (it opens the
  // Network view), with the speed test under it.
  if (NET_WORDS.includes(typed) && !appNamed) {
    const kept = rows.filter((row) => !(row.kind === 'view' && row.target === 'network'));
    if (!kept.some((row) => row.kind === 'view' && row.target === 'speedtest')) {
      kept.unshift(commandRow(COMMANDS.find((command) => command.id === 'speedtest')!));
    }
    rows.splice(0, rows.length, netCard('network'), ...kept);
    preselect = 0;
  }
  // "ip": the card is the local address; the view is a ⌘↵ away.
  if (rows[0]?.kind === 'netCard' && rows[0].target === 'ip') {
    let last = 0;
    rows.forEach((row, index) => {
      if (row.kind === 'answer') last = index;
    });
    rows.splice(
      last + 1,
      0,
      view('network', 'Network', 'glyph:pulse', {
        keyHints: [{ keys: 'mod+enter', label: 'Open' }],
      })
    );
  }
  pastAnswerRows(rows, ctx);
  // An alias the user set, typed exactly, outranks everything, app name matches included.
  const aliased = aliasRow(ctx);
  if (aliased) {
    rows.unshift(aliased);
    preselect = 0;
  }
  return { rows, preselect };
}

function titleFor(row: SuggestionRow): string {
  const section = row.section ?? '';
  if (section === 'card') return '';
  if (section === 'web') {
    if (row.kind === 'remote') return 'Google suggestions';
    return row.target === 'url' ? 'Link' : 'Google';
  }
  return SECTION_TITLES[section] ?? '';
}

/** Splits rows into titled groups; a header starts wherever the section changes. */
export function groups(rows: SuggestionRow[]): Group[] {
  const result: Group[] = [];
  rows.forEach((row, index) => {
    const title = titleFor(row);
    const previous = result[result.length - 1];
    // Late suggestions share the query row's header when it's a Google search.
    const sameWeb =
      previous &&
      row.kind === 'remote' &&
      previous.rows[previous.rows.length - 1]?.section === 'web' &&
      previous.rows[0]?.target === row.target;
    if (previous && (previous.title === title || sameWeb)) {
      previous.rows.push(row);
    } else {
      result.push({ title, start: index, rows: [row] });
    }
  });
  return result;
}

/** The binding an alias names, matched case-insensitively. */
export function bindingForAlias(ctx: RowCtx, alias: string): Binding | undefined {
  const wanted = alias.trim().toLowerCase();
  if (!wanted) return undefined;
  return ctx.snapshot?.bindings?.find((binding) => binding.alias && binding.alias === wanted);
}

/** The command (mode or scoped search) a view: or scope: target names. */
function commandFor(target: string): Command | undefined {
  const id = target.startsWith('view:') ? target.slice(5) : target;
  return COMMANDS.find((command) => command.id === id);
}

/** A row for whatever a binding points at, as the bar would show it. */
function bindingRow(binding: Binding, groups: LaunchGroup[] = []): SuggestionRow | null {
  const [kind, ...rest] = binding.target.split(':');
  const value = rest.join(':');
  const hint = binding.alias ? `Alias “${binding.alias}”` : '';
  switch (kind) {
    case 'app':
      return {
        kind: 'app',
        appId: value,
        label: binding.label,
        query: '',
        target: '',
        hint,
        section: 'apps',
        icon: `app:${value}`,
      };
    case 'bookmark':
      return {
        kind: 'bookmark',
        label: binding.label,
        query: value,
        target: '',
        hint,
        section: 'bookmarks',
        icon: `site:${value}`,
      };
    case 'group': {
      const group = groups.find((item) => item.id === value);
      return group ? groupRow(group, hint) : null;
    }
    case 'view':
    case 'scope': {
      const command = commandFor(binding.target);
      if (!command) return null;
      return view(command.id, command.label, command.icon, { hint });
    }
    default:
      return null;
  }
}

function aliasRow(ctx: RowCtx): SuggestionRow | null {
  const binding = bindingForAlias(ctx, ctx.typed);
  return binding ? bindingRow(binding, ctx.snapshot?.launchGroups) : null;
}

/**
 * The binding target for a row, when the row can have a hotkey or alias: an app, a
 * bookmark, a settings page, or a command (a mode or scoped search).
 */
export function bindingTarget(row: SuggestionRow): { target: string; label: string } | null {
  if (row.kind === 'app' && row.appId) return { target: `app:${row.appId}`, label: row.label };
  if (row.kind === 'bookmark') return { target: `bookmark:${row.query}`, label: row.label };
  if (row.kind === 'setting' && row.settingId) {
    return { target: `setting:${row.settingId}`, label: row.label };
  }
  if (row.kind === 'view') {
    const id = row.target;
    if (id === 'back' || id === 'ip') return null;
    if (id.startsWith('group:')) return { target: id, label: row.label };
    const target = id.startsWith('scope:') ? id : `view:${id}`;
    return commandFor(target) ? { target, label: row.label } : null;
  }
  return null;
}

/** Every mode and scoped search, as binding targets, for Settings > Shortcuts. */
export function commandTargets(): { target: string; label: string }[] {
  return COMMANDS.map((command) => ({
    target: command.id.startsWith('scope:') ? command.id : `view:${command.id}`,
    label: command.label,
  }));
}

/** Under a calculation or conversion, the last few answers copied before it (Enter copies
 *  one). Times aren't kept: they'd be stale. */
function pastAnswerRows(rows: SuggestionRow[], ctx: RowCtx) {
  const at = rows.findIndex(
    (row) =>
      row.kind === 'answer' && (row.icon === 'glyph:calculator' || row.icon === 'glyph:conversion')
  );
  if (at < 0 || !ctx.pastAnswers?.length) return;
  const asked = (rows[at].subtitle ?? ctx.typed).trim();
  const past = ctx.pastAnswers
    .filter((entry) => entry.question !== asked)
    .slice(0, 3)
    .map((entry): SuggestionRow => ({
      kind: 'answer',
      target: 'past',
      label: entry.answer,
      query: entry.answer,
      subtitle: entry.question,
      section: 'answer',
      icon: 'glyph:history',
      hint: 'Copy',
    }));
  rows.splice(at + 1, 0, ...past);
}
