---
title: How the docs work
---

The docs site is a living description of what Zephyr does, kept in sync with the code. Each page records what is shipped, what is partial, and what is still missing (`gaps`). They replace the long feature list in the README and publish to [GitHub Pages](https://z3k-md.github.io/zephyr/) after each release.

## Layout

- `docs/index.md` — home page with download buttons and highlights
- `docs/guide/` — install and how to use the bar
- `docs/features/` — one page per feature
- `docs/roadmap.md` — built at site build time from feature frontmatter
- `docs/contributing/` — develop, branches, release, and this page

VitePress lives under `docs/.vitepress/` (theme, config, `unowned.txt` for the sync check).

## Feature page contract

Feature pages use YAML frontmatter and a fixed body shape.

```yaml
---
title: Clipboard history
summary: Search everything you've copied and paste it into the app in front.
status: shipped # shipped | partial | in-progress | planned
group: utilities # bar | utilities | account
platforms: [windows, macos] # windows | macos | linux; often omitted on planned pages
since: 2.4.0 # first release that included it; only on shipped/partial
code: # repo-relative globs (forward slashes) for source this page describes
  - src-tauri/src/clipboard/**
  - src-tauri/src/commands/clipboard.rs
  - src/bar/ClipboardView.vue
gaps: # known missing pieces; not bugs
  - Sync across computers (waits on Sync)
---
```

**Body sections** (in order):

1. One opening paragraph (no heading): what it does and how to open it.
2. `## Use it` — main flows.
3. `## Keys` — table (`Key | Does`, or `Windows | macOS | Does` when shortcuts differ).
4. `## Settings` — what Settings changes and where.
5. `## Privacy` or `## How it works` — only when it matters.

The theme renders `title` as the page heading. Status, platforms, `since`, and `gaps` come from frontmatter (badge, "Not yet" box).

### Fields

| Field       | Meaning                                                                                                                                                 |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `title`     | Page name in the nav and hero.                                                                                                                          |
| `summary`   | One line for cards and search.                                                                                                                          |
| `status`    | `shipped` — on develop and usable; `partial` — usable but incomplete; `in-progress` — being built; `planned` — not started yet (stub from the roadmap). |
| `group`     | Sidebar section: `bar` (bar and search), `utilities`, or `account`.                                                                                     |
| `platforms` | Where the feature runs today.                                                                                                                           |
| `since`     | First release version (no leading `v`) once a release ships with the feature.                                                                           |
| `code`      | Globs for Rust and frontend files the page documents. Shared shell files go in `docs/.vitepress/unowned.txt`, not on a feature page.                    |
| `gaps`      | Intentional missing work; drives the roadmap and the "Not yet" box.                                                                                     |

Guide pages may use `title`, optional `summary`, and `code` only (for example `guide/the-bar.md` for bar core).

## Rules when you change the app

- If you change what the app **does**, update the matching page in the **same commit**.
- A **new feature** starts as a `docs/features/*.md` page with `status: in-progress`. When it is on develop and usable, set `status: shipped` (or `partial` if gaps remain).
- Fill in `since` once a **release** includes the feature (first tag that contains the main implementation commit, version without `v`).
- A **refactor** with no user-visible change: add a `Docs: none` trailer to the commit message so the sync check does not ask for a doc edit.
- **Bugs** belong in [GitHub Issues](https://github.com/z3k-md/zephyr/issues), not in `gaps`.
- Owners, dates, and priorities for tasks live outside the docs (a tracker later).

## docs-check

`scripts/docs-check.mjs` prints **warnings only** and always exits 0. It checks:

- Changed source matches a page's `code` globs but that page did not change (unless `Docs: none` in the commit range).
- A `code` glob matches no tracked file.
- Changed source under `src/` or `src-tauri/src/` is not covered by any page or `unowned.txt`.
- Invalid feature frontmatter.

Run it locally:

```bash
bun run docs:check          # changes since merge-base with origin/develop
node scripts/docs-check.mjs --all   # whole tree (unowned covers all source)
```

It also runs in `bun run check` (Docs step) and in the **Docs** job on GitHub Actions. That job is **not** part of the required Checks matrix, so it can fail without blocking promotion to main.

## Preview and publishing

- Local preview: `bun run docs:dev`
- After each release, the **Release** workflow's last job builds the site from the release tag and deploys to GitHub Pages, so the site matches the build users install.
- To redeploy by hand: **Actions → Docs → Run workflow**, optionally pass a ref or tag (default: latest release).

**One-time setup:** In the repo settings, set Pages **Source** to **GitHub Actions**. The `github-pages` environment must allow deployments from `main` (Release runs on main).
