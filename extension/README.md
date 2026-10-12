# Zephyr browser extension

Lets Zephyr switch to a tab you already have open instead of opening the page again. When you
open a bookmark from the bar (typing `music`, say) and YouTube Music is already open, its tab
comes to the front.

Works in Chrome, Edge, Brave and other Chromium browsers. Without the extension, bookmarks
still open, just always in a new tab.

## Install

1. Open `chrome://extensions` (or `edge://extensions`).
2. Turn on **Developer mode**.
3. Click **Load unpacked** and choose this `extension` folder.

After these files change, click the reload icon on the extension there.

## How it works

The extension keeps a local WebSocket open to Zephyr (`ws://127.0.0.1:47391`; Zephyr only
accepts browser extensions there). When you open a bookmark, Zephyr first asks it whether a tab
is already on that page: the same address, or for a bookmark to a site or section (no
`?query`), any page under it, so a `music.youtube.com` bookmark matches a song that is playing.
If so, the extension switches to that tab and nothing new opens.

When the link isn't up yet (just after the browser starts), Zephyr opens the page through a
link to itself, `http://127.0.0.1:<port>/zephyr/open?u=<page>`, which redirects to the page.
The extension catches that too, switches to the open tab and closes the new one.

It needs the `tabs`, `webNavigation` and `alarms` permissions, and talks only to Zephyr on this
computer.
