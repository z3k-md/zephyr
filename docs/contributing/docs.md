---
title: How the docs work
---

The site describes what Zephyr does today, one page per feature, and what each feature still lacks. It changes with the code.

## Layout

| Path                 | Holds                                                             |
| -------------------- | ----------------------------------------------------------------- |
| `docs/index.md`      | Home page                                                         |
| `docs/guide/`        | Install and the bar                                               |
| `docs/features/`     | One page per feature                                              |
| `docs/roadmap.md`    | Built from feature frontmatter                                    |
| `docs/contributing/` | Develop, branches, release, this page                             |
| `docs/.vitepress/`   | Config, theme, and `unowned.txt` (source that belongs to no page) |

## Feature pages

```yaml
---
title: Clipboard history
summary: Search everything you've copied and paste it into the app in front.
status: shipped # shipped | partial | in-progress | planned
group: utilities # bar | utilities | account
platforms: [windows, macos] # windows | macos | linux; omit on planned pages
since: 2.4.0 # first release with it; shipped and partial only
code: # globs for the source the page describes
  - src-tauri/src/clipboard/**
  - src-tauri/src/commands/clipboard.rs
  - src/bar/ClipboardView.vue
gaps: # missing pieces, not bugs
  - Clipboard sync
---
```

| Status        | Means                              |
| ------------- | ---------------------------------- |
| `shipped`     | On `develop` and usable            |
| `partial`     | Usable, with gaps                  |
| `in-progress` | Being built; may be off by default |
| `planned`     | Not started                        |

`group` picks the sidebar section. The theme draws the title, summary, status, platforms, `since` and the "Not yet" box from frontmatter, so the body has no H1 and no component tags.

Body, in order, all but the first optional:

1. An opening paragraph: what it does and how to open it.
2. `## Use it`: flows the opening doesn't cover.
3. `## Keys`: a table, `Key | Does` or `Windows | macOS | Does`.
4. `## Settings`: what changes where (`Settings > Section`).
5. `## Privacy` or `## How it works`: only when it matters.

Guide pages take `title`, `summary` and `code` only.

## Rules

- A change to what the app does updates its page in the same commit.
- A new feature starts as a page with `status: in-progress` and becomes `shipped` once it's usable on `develop`.
- `since` is filled in once a release includes the feature: the first tag containing its main commit, without the `v`.
- A refactor with no visible change adds a `Docs: none` trailer to the commit message.
- Bugs go to [GitHub Issues](https://github.com/z3k-md/zephyr/issues), not `gaps`.
- Owners, dates and priorities belong to the task tracker, not the docs.

## docs-check

`scripts/docs-check.mjs` warns and always exits 0. It reports:

- source matching a page's `code` changed, but the page didn't (skipped when a commit in the range has `Docs: none`)
- a `code` glob that matches no file
- a source file under `src/` or `src-tauri/src/` on no page and not in `unowned.txt`
- invalid feature frontmatter

```bash
bun run docs:check                 # changes since the merge base with origin/develop
node scripts/docs-check.mjs --all  # the whole tree
```

It also runs in `bun run check` and in the Docs job on GitHub Actions. That job isn't a required check, so it never blocks promotion.

## Preview and publish

`bun run docs:dev` serves the site locally.

The Release workflow's last job builds the site from the release tag and deploys it to GitHub Pages, so the site matches the installed app. Actions > Docs > Run workflow redeploys by hand, at the latest release or a given ref.

One-time setup: Pages source set to GitHub Actions, and the `github-pages` environment allowing `main`, since Release runs there.
