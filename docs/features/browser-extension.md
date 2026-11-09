---
title: Browser extension
summary: Switch to an open tab instead of duplicating it; block sites during focus sessions.
status: shipped
group: bar
platforms: [windows, macos]
since: 2.6.0
code:
  - extension/**
  - src-tauri/src/browser.rs
  - src-tauri/src/loopback.rs
---

The Chromium extension lets Zephyr switch to a tab you already have open instead of opening the page again. It also enforces site blocking during a focus session when [Timers and focus](/features/timers-and-focus) is configured.

Works in Chrome, Edge, Brave and other Chromium browsers. Without the extension, bookmarks still open, just always in a new tab.

## Use it

Load the unpacked extension from the repo's `extension` folder (install steps live in `extension/README.md`).

When you open a bookmark from the bar, Zephyr asks the extension whether a tab is already on that page: the same address, or for a bookmark to a site or section (no `?query`), any page under it, so a `music.youtube.com` bookmark matches a song that is playing. If so, the extension switches to that tab and nothing new opens.

When the WebSocket is not up yet, Zephyr opens the page through `http://127.0.0.1:<port>/zephyr/open?u=<page>`, which redirects to the page. The extension catches that, switches to the open tab and closes the new one.

## Settings

Focus blocking lists come from **Settings > Focus** (sites to block until the session ends). See [Timers and focus](/features/timers-and-focus).

## How it works

The extension keeps a local WebSocket open to Zephyr (`ws://127.0.0.1:47391`; Zephyr only accepts its own extension there). It needs the `tabs`, `webNavigation`, `alarms` and `storage` permissions, and talks only to Zephyr on this computer.

During a focus session Zephyr also sends the sites to block and when the session ends (`{type:"focusBlock", sites, until}`). Until then, a top-frame navigation to one of those sites, or any subdomain of it, opens a blocked page with the time left. Tabs already open on those sites are sent there too. The block is kept in the browser's session storage, so it still applies if the extension's worker restarts before the session ends. Without the extension the session still runs; those sites just stay open.

The `key` in the manifest gives the extension a fixed id (`akdgnalpenplabkcifocffknemieldni`), the only one Zephyr accepts.
