// Warn-only sync between source changes and VitePress pages (frontmatter code globs).
// Always exits 0; CI uses GitHub warning annotations when GITHUB_ACTIONS is set.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const DOC_ROOT = 'docs';
const FEATURE_PREFIX = `${DOC_ROOT}/features/`;

const VALID_STATUS = new Set(['shipped', 'partial', 'in-progress', 'planned']);
const VALID_GROUP = new Set(['bar', 'utilities', 'account']);
const VALID_PLATFORMS = new Set(['windows', 'macos', 'linux']);
const SINCE_RE = /^\d+\.\d+\.\d+$/;
const SOURCE_PREFIXES = ['src/', 'src-tauri/src/'];

/** Escape a glob segment for regex (no /, *, ?, { handled elsewhere). */
function escapeRegexLiteral(text) {
  return text.replace(/[\\^$+.()|[\]{}]/g, '\\$&');
}

/** One path segment: *, ?, and {a,b}. */
function segmentToRegex(segment) {
  let out = '';
  for (let i = 0; i < segment.length;) {
    if (segment[i] === '{') {
      const end = segment.indexOf('}', i);
      if (end === -1) return escapeRegexLiteral(segment);
      const alts = segment
        .slice(i + 1, end)
        .split(',')
        .map((part) => segmentToRegex(part));
      out += `(?:${alts.join('|')})`;
      i = end + 1;
      continue;
    }
    const ch = segment[i++];
    if (ch === '*') out += '[^/]*';
    else if (ch === '?') out += '[^/]';
    else out += escapeRegexLiteral(ch);
  }
  return out;
}

/** Repo-relative glob: ** (any depth, including zero), *, ?, {a,b}, forward slashes. */
export function globToRegExp(glob) {
  let regex = '^';
  let i = 0;
  while (i < glob.length) {
    if (glob[i] === '*' && glob[i + 1] === '*') {
      const after = glob[i + 2];
      if (after === '/') {
        regex += '(?:.*/)?';
        i += 3;
      } else if (i === 0 && i + 2 >= glob.length) {
        regex += '.*';
        i += 2;
      } else if (i + 2 >= glob.length) {
        if (regex.endsWith('/')) regex += '.*';
        else if (i === 0) regex += '.*';
        else regex += '(?:/.*)?';
        i += 2;
      } else {
        regex += '(?:.*/)';
        i += 2;
      }
      continue;
    }
    const slash = glob.indexOf('/', i);
    const segment = slash === -1 ? glob.slice(i) : glob.slice(i, slash);
    regex += segmentToRegex(segment);
    if (slash === -1) i = glob.length;
    else {
      regex += '/';
      i = slash + 1;
    }
  }
  regex += '$';
  return new RegExp(regex);
}

export function matchGlob(repoPath, glob) {
  return globToRegExp(glob.replace(/\\/g, '/')).test(repoPath.replace(/\\/g, '/'));
}

export function matchesAnyGlob(repoPath, globs) {
  const normalized = repoPath.replace(/\\/g, '/');
  return globs.some((g) => matchGlob(normalized, g));
}

export function parseUnownedLines(text) {
  const globs = [];
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.replace(/\s+#.*$/, '').trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    globs.push(trimmed);
  }
  return globs;
}

export function hasDocsNoneTrailer(trailerLines) {
  return trailerLines.some((line) => line.trim().toLowerCase() === 'none');
}

/** @returns {{ file: string, message: string }[]} */
export function warningsDeadGlobs(trackedFiles, pages) {
  const tracked = new Set(trackedFiles.map((p) => p.replace(/\\/g, '/')));
  const warnings = [];
  for (const page of pages) {
    for (const glob of page.codeGlobs) {
      const hit = [...tracked].some((path) => matchGlob(path, glob));
      if (!hit) {
        warnings.push({
          file: page.path,
          message: `code glob matches no tracked file: ${glob}`,
        });
      }
    }
  }
  return warnings;
}

/** @returns {{ file: string, message: string }[]} */
export function warningsUndocumented(changedFiles, changedPages, pages, skipUndocumented) {
  if (skipUndocumented) return [];
  const changed = new Set(changedFiles.map((p) => p.replace(/\\/g, '/')));
  const changedPageSet = new Set(changedPages.map((p) => p.replace(/\\/g, '/')));
  const warnings = [];

  for (const page of pages) {
    if (!page.codeGlobs.length) continue;
    const hits = [];
    for (const path of changed) {
      if (matchesAnyGlob(path, page.codeGlobs)) hits.push(path);
    }
    if (!hits.length) continue;
    if (changedPageSet.has(page.path.replace(/\\/g, '/'))) continue;
    const sample = hits.slice(0, 5);
    const extra = hits.length > 5 ? ` (+${hits.length - 5} more)` : '';
    warnings.push({
      file: page.path,
      message: `source changed but this page did not (${sample.join(', ')}${extra})`,
    });
  }
  return warnings;
}

function isSourcePath(repoPath) {
  const p = repoPath.replace(/\\/g, '/');
  return SOURCE_PREFIXES.some((prefix) => p.startsWith(prefix));
}

/** @returns {{ file: string, message: string }[]} */
export function warningsUnowned(candidateFiles, pages, unownedGlobs) {
  const pageGlobs = pages.flatMap((p) => p.codeGlobs);
  const warnings = [];
  for (const raw of candidateFiles) {
    const path = raw.replace(/\\/g, '/');
    if (!isSourcePath(path)) continue;
    if (matchesAnyGlob(path, pageGlobs)) continue;
    if (matchesAnyGlob(path, unownedGlobs)) continue;
    warnings.push({
      file: path,
      message: "source file isn't on any page's code list or in docs/.vitepress/unowned.txt",
    });
  }
  return warnings;
}

function isStringArray(value) {
  return Array.isArray(value) && value.every((v) => typeof v === 'string');
}

/** @returns {{ file: string, message: string }[]} */
export function warningsFeatureFrontmatter(pages) {
  const warnings = [];
  for (const page of pages) {
    if (!page.isFeature) continue;
    const data = page.data ?? {};
    const base = page.path;

    const need = (field) => {
      if (data[field] === undefined || data[field] === null || data[field] === '') {
        warnings.push({ file: base, message: `missing frontmatter field: ${field}` });
      }
    };
    need('title');
    need('summary');
    need('status');
    need('group');

    if (data.status !== undefined && !VALID_STATUS.has(data.status)) {
      warnings.push({
        file: base,
        message: `status must be shipped, partial, in-progress, or planned (got ${JSON.stringify(data.status)})`,
      });
    }

    if (data.group !== undefined && !VALID_GROUP.has(data.group)) {
      warnings.push({
        file: base,
        message: `group must be bar, utilities, or account (got ${JSON.stringify(data.group)})`,
      });
    }

    if (data.platforms !== undefined) {
      if (!isStringArray(data.platforms)) {
        warnings.push({ file: base, message: 'platforms must be a list of strings' });
      } else {
        const bad = data.platforms.filter((p) => !VALID_PLATFORMS.has(p));
        if (bad.length) {
          warnings.push({
            file: base,
            message: `unknown platform(s): ${bad.join(', ')}`,
          });
        }
      }
    }

    const status = data.status;
    if (status === 'planned' || status === 'in-progress') {
      if (data.since !== undefined && data.since !== null && data.since !== '') {
        warnings.push({
          file: base,
          message: 'since must not be set on planned or in-progress pages',
        });
      }
    } else if (status === 'shipped' || status === 'partial') {
      if (data.since !== undefined && data.since !== null && data.since !== '') {
        if (typeof data.since !== 'string' || !SINCE_RE.test(data.since)) {
          warnings.push({ file: base, message: 'since must be a version like 2.4.0' });
        }
      }
    }

    if (status === 'shipped' || status === 'partial' || status === 'in-progress') {
      const code = data.code;
      if (!code || (Array.isArray(code) && code.length === 0)) {
        warnings.push({ file: base, message: 'code globs are required for this status' });
      }
    }

    if (data.code !== undefined && data.code !== null && !isStringArray(data.code)) {
      warnings.push({ file: base, message: 'code must be a list of glob strings' });
    }

    if (data.gaps !== undefined && data.gaps !== null && !isStringArray(data.gaps)) {
      warnings.push({ file: base, message: 'gaps must be a list of strings' });
    }
  }
  return warnings;
}

export function formatWarning(file, message) {
  const safeFile = (file ?? '').replace(/%/g, '%25').replace(/\r/g, '%0D').replace(/\n/g, '%0A');
  const safeMsg = message.replace(/%/g, '%25').replace(/\r/g, '%0D').replace(/\n/g, '%0A');
  if (process.env.GITHUB_ACTIONS === 'true') {
    return safeFile
      ? `::warning file=${safeFile},title=docs-check::${safeMsg}`
      : `::warning title=docs-check::${safeMsg}`;
  }
  return file ? `docs-check: ${file}: ${message}` : `docs-check: ${message}`;
}

function runGit(args, cwd = ROOT) {
  const result = spawnSync('git', args, { cwd, encoding: 'utf8' });
  if (result.status !== 0 && result.stderr) {
    throw new Error(result.stderr.trim() || `git ${args.join(' ')} failed`);
  }
  return (result.stdout ?? '').trimEnd();
}

function listTrackedFiles() {
  const tracked = runGit(['ls-files']).split('\n').filter(Boolean);
  const untracked = runGit(['ls-files', '--others', '--exclude-standard'])
    .split('\n')
    .filter(Boolean);
  return [...new Set([...tracked, ...untracked])];
}

/** Where this branch left `ref`, so commits that landed on `ref` since don't count as changes. */
function mergeBase(ref) {
  try {
    return runGit(['merge-base', 'HEAD', ref]) || ref;
  } catch {
    return ref === 'origin/develop' ? 'HEAD' : ref;
  }
}

function parseArgs(argv) {
  let base = null;
  let all = false;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--all') all = true;
    else if (arg === '--base') {
      base = argv[++i];
      if (!base) throw new Error('--base requires a ref');
    } else if (arg === '--help' || arg === '-h') {
      console.log('usage: node scripts/docs-check.mjs [--all] [--base <ref>]');
      process.exit(0);
    } else throw new Error(`unknown option: ${arg}`);
  }
  return { base, all };
}

function changedFilesSince(base) {
  const diff = runGit(['diff', '--name-only', base]).split('\n').filter(Boolean);
  const untracked = runGit(['ls-files', '--others', '--exclude-standard'])
    .split('\n')
    .filter(Boolean);
  return [...new Set([...diff, ...untracked])];
}

function docsNoneInRange(base) {
  try {
    const out = runGit(['log', `--format=%(trailers:key=Docs,valueonly)`, `${base}..HEAD`]);
    const lines = out ? out.split('\n') : [];
    return hasDocsNoneTrailer(lines);
  } catch {
    return false;
  }
}

function walkDocMdFiles() {
  const paths = [];
  function walk(absDir, prefix) {
    for (const name of readdirSync(absDir, { withFileTypes: true })) {
      if (name.name === '.vitepress' || name.name === 'node_modules') continue;
      const rel = prefix ? `${prefix}/${name.name}` : name.name;
      const abs = join(absDir, name.name);
      if (name.isDirectory()) walk(abs, rel);
      else if (name.name.endsWith('.md')) paths.push(`${DOC_ROOT}/${rel}`.replace(/\\/g, '/'));
    }
  }
  const docsDir = join(ROOT, DOC_ROOT);
  if (existsSync(docsDir)) walk(docsDir, '');
  return paths;
}

function loadPages(parseFrontmatter) {
  const paths = walkDocMdFiles();
  const pages = [];
  for (const path of paths) {
    const abs = join(ROOT, path);
    let data;
    let parseError = null;
    try {
      const raw = readFileSync(abs, 'utf8');
      const parsed = parseFrontmatter(raw);
      data = parsed.data ?? {};
    } catch (err) {
      parseError = err;
      data = {};
    }
    const code = data.code;
    const codeGlobs = isStringArray(code) ? [...code] : [];
    pages.push({
      path,
      isFeature: path.startsWith(FEATURE_PREFIX),
      data,
      codeGlobs,
      parseError,
    });
  }
  return pages;
}

export async function main(argv = process.argv.slice(2)) {
  const warnings = [];
  try {
    const { base: baseArg, all } = parseArgs(argv);
    const matter = (await import('gray-matter')).default;

    const parseFrontmatter = (raw) => matter(raw);

    const pages = loadPages(parseFrontmatter);
    for (const page of pages) {
      if (page.parseError) {
        warnings.push({
          file: page.path,
          message: `frontmatter parse error: ${page.parseError.message}`,
        });
      }
    }

    warnings.push(...warningsFeatureFrontmatter(pages));

    const tracked = listTrackedFiles();
    warnings.push(...warningsDeadGlobs(tracked, pages));

    const base = all ? 'HEAD' : mergeBase(baseArg ?? 'origin/develop');
    const changed = all ? [] : changedFilesSince(base);
    const changedPages = changed.filter((p) => p.replace(/\\/g, '/').startsWith(`${DOC_ROOT}/`));
    const skipUndocumented = all || docsNoneInRange(base);

    warnings.push(...warningsUndocumented(changed, changedPages, pages, skipUndocumented));

    const unownedPath = join(ROOT, DOC_ROOT, '.vitepress', 'unowned.txt');
    let unownedGlobs = [];
    if (existsSync(unownedPath)) {
      unownedGlobs = parseUnownedLines(readFileSync(unownedPath, 'utf8'));
    }

    const trackedSet = new Set(tracked);
    const unownedCandidates = all ? tracked : changed.filter((p) => trackedSet.has(p));
    warnings.push(...warningsUnowned(unownedCandidates, pages, unownedGlobs));
  } catch (err) {
    warnings.push({ file: '', message: `check did not finish: ${err.message ?? err}` });
  }

  for (const w of warnings) {
    console.log(formatWarning(w.file, w.message));
  }
  if (warnings.length) {
    console.log(`docs-check: ${warnings.length} warning${warnings.length === 1 ? '' : 's'}`);
  } else {
    console.log('docs-check: no warnings');
  }
}

const isMain =
  process.argv[1] &&
  fileURLToPath(import.meta.url) === fileURLToPath(pathToFileURL(process.argv[1]));
if (isMain) {
  main().then(
    () => process.exit(0),
    (err) => {
      console.log(formatWarning('', `check did not finish: ${err.message ?? err}`));
      process.exit(0);
    }
  );
}
