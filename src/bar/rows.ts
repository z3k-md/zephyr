// Turns a suggest response into the bar's sectioned list: the query row, jump-in rows and
// section headers. Pure functions, so the bar only wires them up.

import { CTRL, MOD, shortcutKeys } from '../keys';
import type { ClaudeJob, Destination, Snapshot, Suggestion, SuggestResponse } from '../types';

export interface RowCtx {
  snapshot: Snapshot | null;
  armedId: string;
  jobs: ClaudeJob[];
  /** The parked workspace ("Back to …"), if any. */
  parked: string;
  parkedName: string;
  /** Ids behind Ctrl+1..8. */
  strip: string[];
}

export interface Group {
  title: string;
  start: number;
  rows: Suggestion[];
}

const SECTION_TITLES: Record<string, string> = {
  apps: 'Apps',
  answer: 'Answer',
  recent: 'Recent',
  files: 'Files',
  settings: 'Settings',
  notes: 'Notes',
  destinations: 'Destinations',
  jump: 'Jump in',
  searchWith: 'Search with',
  shell: 'Shell',
};

function destination(ctx: RowCtx, id: string): Destination | undefined {
  return ctx.snapshot?.destinations.find((item) => item.id === id);
}

function destinationName(ctx: RowCtx, id: string): string {
  if (id === 'url') return 'Link';
  return destination(ctx, id)?.name ?? id;
}

export function rowKey(row: Suggestion): string {
  if (row.kind === 'query') return 'query';
  if (row.kind === 'searchWith') return `sw-${row.destinationId}`;
  if (row.kind === 'more') return 'more';
  return `${row.kind}-${row.destinationId}-${row.path ?? row.noteId ?? row.label}`;
}

function view(id: string, label: string, icon: string, extra: Partial<Suggestion>): Suggestion {
  return {
    label,
    hint: '',
    query: '',
    destinationId: id,
    kind: 'view',
    section: 'jump',
    icon,
    ...extra,
  };
}

/** Rows on the empty bar that jump straight into a view. */
export function jumpRows(ctx: RowCtx): Suggestion[] {
  const rows: Suggestion[] = [];
  const settings = ctx.snapshot;
  if (ctx.parked && ctx.parkedName) {
    rows.push(
      view('back', `Back to ${ctx.parkedName}`, 'glyph:back', { hint: 'Esc again to clear' })
    );
  }
  if (settings?.clipboard.enabled) {
    const keys = settings.clipboard.shortcut ? shortcutKeys(settings.clipboard.shortcut) : [];
    rows.push(
      view(
        'clip',
        'Clipboard history',
        'glyph:clipboard',
        keys.length ? { keys } : { hint: '!clip' }
      )
    );
  }
  const noteKeys = settings?.notesShortcut ? shortcutKeys(settings.notesShortcut) : [];
  rows.push(
    view('notes', 'Notes', 'glyph:note', noteKeys.length ? { keys: noteKeys } : { hint: '!note' })
  );
  if (ctx.jobs.length || settings?.claude.projects.length) {
    const running = ctx.jobs.filter(
      (job) => job.status === 'running' || job.status === 'queued'
    ).length;
    const waiting = ctx.jobs.filter((job) => job.status === 'waiting').length;
    const parts = [running ? `${running} running` : '', waiting ? `${waiting} waiting` : ''].filter(
      Boolean
    );
    const latest = ctx.jobs[0];
    const hint = parts.length
      ? parts.join(', ')
      : latest
        ? `${latest.status === 'failed' ? '✗' : '✓'} ${latest.title}`
        : '!claude';
    rows.push(view('claude', 'Claude jobs', 'glyph:terminal', { hint }));
  }
  return rows;
}

/** The rows the bar lists, and which one Enter takes before the user moves. */
export function buildRows(
  response: SuggestResponse,
  ctx: RowCtx
): { rows: Suggestion[]; preselect: number | null } {
  if (response.mode === 'recent' && !response.text) {
    return { rows: [...jumpRows(ctx), ...response.items], preselect: null };
  }
  const rows = [...response.items];
  let preselect = response.preselect;
  // Free text going to a destination gets its own row at the top, selected.
  if (
    response.mode === 'search' &&
    response.target &&
    response.preselect === null &&
    response.text
  ) {
    const target = response.target;
    const ai = destination(ctx, target)?.kind === 'ai';
    rows.unshift({
      kind: 'query',
      label: response.text,
      query: response.text,
      destinationId: target,
      section: 'web',
      icon: target === 'url' ? 'glyph:link' : `dest:${target}`,
      hint: target === 'url' ? 'Open' : ai ? 'Ask' : 'Search',
    });
    preselect = 0;
  }
  if (response.mode === 'search' && response.target && response.text) {
    rows.push(...searchWithRows(response, ctx, rows[0]?.kind === 'query' ? response.target : null));
  }
  return { rows, preselect };
}

/** "Search with": three other destinations for the same text, then "N more". */
function searchWithRows(
  response: SuggestResponse,
  ctx: RowCtx,
  exclude: string | null
): Suggestion[] {
  let ids = response.searchWith.filter((id) => id !== exclude);
  if (!exclude && ids.includes(ctx.armedId)) {
    ids = [ctx.armedId, ...ids.filter((id) => id !== ctx.armedId)];
  }
  const shown = ids.slice(0, 3).map((id): Suggestion => {
    const index = ctx.strip.indexOf(id);
    const trigger = destination(ctx, id)?.triggers[0];
    return {
      kind: 'searchWith',
      section: 'searchWith',
      label: destinationName(ctx, id),
      query: response.text,
      subtitle: response.text,
      destinationId: id,
      icon: `dest:${id}`,
      keys: index >= 0 ? [CTRL, String(index + 1)] : undefined,
      hint: index < 0 && trigger ? `!${trigger}` : '',
    };
  });
  const remaining = ids.length - shown.length;
  if (remaining > 0) {
    shown.push({
      kind: 'more',
      section: 'searchWith',
      label: `${remaining} more destination${remaining === 1 ? '' : 's'}`,
      query: response.text,
      destinationId: '',
      hint: '',
      icon: 'glyph:more',
      keys: [MOD, 'K'],
    });
  }
  return shown;
}

function titleFor(row: Suggestion, ctx: RowCtx): string {
  const section = row.section ?? '';
  if (section === 'web') {
    const name = destinationName(ctx, row.destinationId);
    return row.kind === 'remote' ? `${name} suggestions` : name;
  }
  return SECTION_TITLES[section] ?? '';
}

/** Splits rows into titled groups; a header starts wherever the section changes. */
export function groups(rows: Suggestion[], ctx: RowCtx): Group[] {
  const result: Group[] = [];
  rows.forEach((row, index) => {
    const title = titleFor(row, ctx);
    const previous = result[result.length - 1];
    // Late suggestions share the query row's header when they're for the same destination.
    const sameWeb =
      previous &&
      row.kind === 'remote' &&
      previous.rows[previous.rows.length - 1]?.section === 'web' &&
      previous.rows[0]?.destinationId === row.destinationId;
    if (previous && (previous.title === title || sameWeb)) {
      previous.rows.push(row);
    } else {
      result.push({ title, start: index, rows: [row] });
    }
  });
  return result;
}

/** What Enter does with the selected row, for the footer. */
export function primaryLabel(row: Suggestion | undefined, ctx: RowCtx): string | null {
  if (!row) return null;
  const name = destinationName(ctx, row.destinationId);
  const isAi = destination(ctx, row.destinationId)?.kind === 'ai';
  switch (row.kind) {
    case 'query':
    case 'remote':
    case 'history':
    case 'searchWith':
      if (row.destinationId === 'url') return 'Open link';
      return isAi ? 'Ask AI' : `Search ${name}`;
    case 'app':
    case 'file':
    case 'setting':
      return `Open ${row.label}`;
    case 'answer':
      return `Copy ${row.label}`;
    case 'note':
      return 'Open note';
    case 'noteNew':
      return 'Create note';
    case 'view':
      return `Open ${row.label.replace(/^Back to /, '')}`;
    case 'more':
      return 'Show all destinations';
    case 'destination':
      return `Use ${row.label}`;
  }
  return null;
}
