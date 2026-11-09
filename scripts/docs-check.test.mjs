import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  globToRegExp,
  hasDocsNoneTrailer,
  matchGlob,
  parseUnownedLines,
  warningsDeadGlobs,
  warningsFeatureFrontmatter,
  warningsUndocumented,
  warningsUnowned,
} from './docs-check.mjs';

test('glob ** at end matches zero or more segments', () => {
  assert.ok(matchGlob('src/foo.ts', 'src/**'));
  assert.ok(matchGlob('src/a/b.ts', 'src/**'));
  assert.ok(!matchGlob('other/foo.ts', 'src/**'));
});

test('glob ** in the middle', () => {
  assert.ok(matchGlob('src-tauri/src/foo/bar.rs', 'src-tauri/src/**/bar.rs'));
  assert.ok(matchGlob('src-tauri/src/bar.rs', 'src-tauri/src/**/bar.rs'));
});

test('glob * and ? within a segment', () => {
  assert.ok(matchGlob('src/log.ts', 'src/log*.ts'));
  assert.ok(matchGlob('src/log.test.ts', 'src/log*.ts'));
  assert.ok(!matchGlob('src/bar/log.ts', 'src/log*.ts'));
  assert.ok(matchGlob('src/a.ts', 'src/?.ts'));
  assert.ok(!matchGlob('src/ab.ts', 'src/?.ts'));
});

test('glob brace alternation', () => {
  assert.ok(matchGlob('src/components/Keys.vue', 'src/components/{InfoTip,Keys}.vue'));
  assert.ok(matchGlob('src/components/InfoTip.vue', 'src/components/{InfoTip,Keys}.vue'));
  assert.ok(!matchGlob('src/components/Marquee.vue', 'src/components/{InfoTip,Keys}.vue'));
});

test('glob does not cross slashes with single *', () => {
  const re = globToRegExp('src/*.css');
  assert.ok(re.test('src/styles.css'));
  assert.ok(!re.test('src/bar/bar.css'));
});

test('hasDocsNoneTrailer', () => {
  assert.equal(hasDocsNoneTrailer(['', 'none', '']), true);
  assert.equal(hasDocsNoneTrailer(['None']), true);
  assert.equal(hasDocsNoneTrailer(['something']), false);
});

test('parseUnownedLines skips comments and blanks', () => {
  assert.deepEqual(parseUnownedLines('# header\n\nsrc/a.ts\n'), ['src/a.ts']);
});

test('warningsDeadGlobs', () => {
  const pages = [{ path: 'docs/features/x.md', codeGlobs: ['src/missing/**'] }];
  const w = warningsDeadGlobs(['src/real.ts'], pages);
  assert.equal(w.length, 1);
  assert.match(w[0].message, /no tracked file/);
});

test('warningsUndocumented groups by page', () => {
  const pages = [
    {
      path: 'docs/features/a.md',
      codeGlobs: ['src/foo.ts', 'src/bar.ts'],
    },
  ];
  const w = warningsUndocumented(['src/foo.ts', 'src/bar.ts'], [], pages, false);
  assert.equal(w.length, 1);
  assert.equal(w[0].file, 'docs/features/a.md');
  assert.match(w[0].message, /src\/foo\.ts/);
});

test('warningsUndocumented skipped when Docs none', () => {
  const pages = [{ path: 'docs/features/a.md', codeGlobs: ['src/foo.ts'] }];
  assert.equal(warningsUndocumented(['src/foo.ts'], [], pages, true).length, 0);
});

test('warningsUndocumented quiet when page changed', () => {
  const pages = [{ path: 'docs/features/a.md', codeGlobs: ['src/foo.ts'] }];
  const w = warningsUndocumented(['src/foo.ts'], ['docs/features/a.md'], pages, false);
  assert.equal(w.length, 0);
});

test('warningsUnowned respects page and unowned globs', () => {
  const pages = [{ path: 'docs/features/a.md', codeGlobs: ['src/owned.ts'] }];
  const w = warningsUnowned(['src/owned.ts', 'src/orphan.ts'], pages, ['src-tauri/src/lib.rs']);
  assert.equal(w.length, 1);
  assert.equal(w[0].file, 'src/orphan.ts');
});

test('warningsFeatureFrontmatter shipped page', () => {
  const pages = [
    {
      path: 'docs/features/x.md',
      isFeature: true,
      data: {
        title: 'T',
        summary: 'S',
        status: 'shipped',
        group: 'bar',
        code: ['src/x.ts'],
        gaps: ['a gap'],
      },
    },
  ];
  assert.equal(warningsFeatureFrontmatter(pages).length, 0);
});

test('warningsFeatureFrontmatter catches bad status and missing code', () => {
  const pages = [
    {
      path: 'docs/features/x.md',
      isFeature: true,
      data: {
        title: 'T',
        summary: 'S',
        status: 'shipped',
        group: 'bar',
        platforms: ['dos'],
        since: 'not-a-version',
        gaps: 1,
      },
    },
  ];
  const w = warningsFeatureFrontmatter(pages);
  assert.ok(w.some((x) => x.message.includes('platform')));
  assert.ok(w.some((x) => x.message.includes('code globs')));
  assert.ok(w.some((x) => x.message.includes('gaps')));
});

test('warningsFeatureFrontmatter since on planned', () => {
  const pages = [
    {
      path: 'docs/features/x.md',
      isFeature: true,
      data: {
        title: 'T',
        summary: 'S',
        status: 'planned',
        group: 'bar',
        since: '1.0.0',
      },
    },
  ];
  assert.ok(warningsFeatureFrontmatter(pages).some((x) => x.message.includes('since must not')));
});
