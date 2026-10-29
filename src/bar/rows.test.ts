import { describe, expect, it } from 'vitest';
import type { Binding, Snapshot, Suggestion, SuggestResponse } from '../types';
import { bindingForAlias, bindingTarget, buildRows, groups, type RowCtx } from './rows';

function snapshot(extra: Partial<Snapshot> = {}): Snapshot {
  return {
    clipboard: { enabled: true, shortcut: '' },
    notesShortcut: '',
    viewOpens: [],
    launchGroups: [],
    bindings: [],
    ...extra,
  } as unknown as Snapshot;
}

function ctx(typed: string, extra: Partial<RowCtx> = {}): RowCtx {
  return { snapshot: snapshot(), parked: '', parkedName: '', typed, ...extra };
}

function row(kind: Suggestion['kind'], label: string, extra: Partial<Suggestion> = {}) {
  return { kind, label, query: label, target: '', hint: '', ...extra } as Suggestion;
}

function response(extra: Partial<SuggestResponse> = {}): SuggestResponse {
  return {
    mode: 'search',
    items: [],
    notice: null,
    preselect: null,
    target: null,
    text: '',
    ...extra,
  };
}

const labels = (rows: Suggestion[]) => rows.map((item) => item.label);

describe('buildRows', () => {
  it('lists recent items, then views, on the empty bar', () => {
    const recent = row('app', 'Firefox', { appId: 'firefox', section: 'recent' });
    const { rows, preselect } = buildRows(response({ mode: 'recent', items: [recent] }), ctx(''));
    expect(preselect).toBeNull();
    expect(labels(rows)).toEqual([
      'Firefox',
      'Clipboard history',
      'Notes',
      'Network',
      'IP address',
    ]);
  });

  it('offers a parked workspace first among the views', () => {
    const { rows } = buildRows(
      response({ mode: 'recent' }),
      ctx('', { parked: 'shell', parkedName: 'Shell' })
    );
    expect(rows[0]).toMatchObject({ kind: 'view', target: 'back', label: 'Back to Shell' });
  });

  it.each([
    ['web search', 'web', 'glyph:google', 'Search · AI Mode'],
    ['link', 'url', 'glyph:link', 'Open'],
  ])('puts a %s row on top, selected', (_, target, icon, hint) => {
    const { rows, preselect } = buildRows(
      response({ text: 'hello', target, items: [row('remote', 'hello world')] }),
      ctx('hello')
    );
    expect(preselect).toBe(0);
    expect(rows[0]).toMatchObject({ kind: 'query', label: 'hello', target, icon, hint });
    expect(rows[1].label).toBe('hello world');
  });

  it('puts the web row under a settings page that leads', () => {
    const page = row('setting', 'Wi-Fi settings', { settingId: 'wifi' });
    const { rows } = buildRows(
      response({ text: 'wifi', target: 'web', preselect: 0, items: [page] }),
      ctx('wifi')
    );
    expect(rows.map((item) => item.kind)).toEqual(['setting', 'query']);
  });

  it.each([
    ['typing', 'Typing test', 0],
    ['typ', 'Typing test', null],
    ['clipboard', 'Clipboard history', 0],
  ])('offers commands by name: %s', (typed, label, preselect) => {
    const { rows, preselect: chosen } = buildRows(response({ text: typed }), ctx(typed));
    expect(labels(rows)).toContain(label);
    expect(chosen).toBe(preselect);
  });

  it('leaves the clipboard out while it is off', () => {
    const off = ctx('clipboard', {
      snapshot: snapshot({ clipboard: { enabled: false } as Snapshot['clipboard'] }),
    });
    const { rows } = buildRows(response({ text: 'clipboard' }), off);
    expect(labels(rows)).not.toContain('Clipboard history');
  });

  it('lets an app named exactly keep Enter over a command', () => {
    const terminal = row('app', 'Terminal', { appId: 'terminal' });
    const { rows, preselect } = buildRows(
      response({ text: 'terminal', items: [terminal], preselect: 0 }),
      ctx('terminal')
    );
    expect(rows[0].label).toBe('Terminal');
    expect(preselect).toBe(0);
  });

  it('shows the network card for "ping", with the speed test under it', () => {
    const { rows, preselect } = buildRows(response({ text: 'ping' }), ctx('ping'));
    expect(rows[0]).toMatchObject({ kind: 'netCard', target: 'network' });
    expect(rows[1]).toMatchObject({ kind: 'view', target: 'speedtest' });
    expect(preselect).toBe(0);
  });

  it('puts an alias the user set above everything', () => {
    const binding: Binding = {
      target: 'app:code',
      label: 'VS Code',
      hotkey: '',
      alias: 'vs',
      direct: false,
    };
    const { rows, preselect } = buildRows(
      response({ text: 'vs', target: 'web' }),
      ctx('vs', { snapshot: snapshot({ bindings: [binding] }) })
    );
    expect(rows[0]).toMatchObject({ kind: 'app', appId: 'code', hint: 'Alias “vs”' });
    expect(preselect).toBe(0);
  });
});

describe('groups', () => {
  it('starts a header wherever the section changes', () => {
    const rows = [
      row('app', 'A', { section: 'apps' }),
      row('app', 'B', { section: 'apps' }),
      row('file', 'C', { section: 'files' }),
    ];
    const found = groups(rows).map((group) => [group.title, group.start, group.rows.length]);
    expect(found).toEqual([
      ['Apps', 0, 2],
      ['Files', 2, 1],
    ]);
  });

  it('keeps Google suggestions under the search row', () => {
    const rows = [
      row('query', 'cat', { section: 'web', target: 'web' }),
      row('remote', 'cats', { section: 'web', target: 'web' }),
    ];
    expect(groups(rows)).toHaveLength(1);
    expect(groups(rows)[0].title).toBe('Google');
  });

  it('gives cards no title', () => {
    expect(groups([row('netCard', 'Network', { section: 'card' })])[0].title).toBe('');
  });
});

describe('bindingForAlias', () => {
  const binding: Binding = {
    target: 'view:shell',
    label: 'Shell',
    hotkey: '',
    alias: 'sh',
    direct: false,
  };
  const withBinding = ctx('', { snapshot: snapshot({ bindings: [binding] }) });

  it.each([
    ['sh', true],
    [' SH ', true],
    ['shell', false],
    ['', false],
  ])('%j finds it: %s', (alias, found) => {
    expect(bindingForAlias(withBinding, alias)).toBe(found ? binding : undefined);
  });

  it('finds nothing without settings', () => {
    expect(bindingForAlias(ctx('', { snapshot: null }), 'sh')).toBeUndefined();
  });
});

describe('bindingTarget', () => {
  it.each([
    [row('app', 'Code', { appId: 'code' }), 'app:code'],
    [row('bookmark', 'News', { query: 'https://news.example' }), 'bookmark:https://news.example'],
    [row('setting', 'Wi-Fi', { settingId: 'wifi' }), 'setting:wifi'],
    [row('view', 'Shell', { target: 'shell' }), 'view:shell'],
    [row('view', 'Search files', { target: 'scope:files' }), 'scope:files'],
    [row('view', 'Work', { target: 'group:work' }), 'group:work'],
    [row('view', 'Back', { target: 'back' }), null],
    [row('view', 'IP address', { target: 'ip' }), null],
    [row('file', 'notes.txt', { path: '/notes.txt' }), null],
  ])('%#: %s', (item, target) => {
    expect(bindingTarget(item)?.target ?? null).toBe(target);
  });
});
