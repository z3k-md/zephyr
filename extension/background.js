// The app opens bookmarks through its own local hand-off link,
// http://127.0.0.1:<port>/zephyr/open?u=<page>. When a tab is already on that page, this
// switches to it and closes the hand-off tab; otherwise the link redirects to the page as
// it would without the extension.

const HANDOFF = /^http:\/\/127\.0\.0\.1:\d+\/zephyr\/open\?/;

/** The page a hand-off link is for. */
function wantedPage(link) {
  try {
    return new URL(link).searchParams.get('u');
  } catch {
    return null;
  }
}

/** The address without its #fragment or a trailing slash. */
function plain(address) {
  const url = new URL(address);
  url.hash = '';
  return url.href.replace(/\/$/, '');
}

/**
 * Whether an open tab counts as the page: the same address, or, for a bookmark to a site or
 * a section of it (no ?query), any page under it. music.youtube.com matches a playing song.
 */
export function sameTab(tabAddress, page) {
  try {
    const tab = new URL(tabAddress);
    const wanted = new URL(page);
    if (tab.origin !== wanted.origin) return false;
    if (plain(tabAddress) === plain(page)) return true;
    if (wanted.search) return false;
    const section = wanted.pathname.replace(/\/$/, '');
    return section === '' || tab.pathname === section || tab.pathname.startsWith(`${section}/`);
  } catch {
    return false;
  }
}

/** Brings the tab already on `page` to the front; false when none is (or only `except` is). */
async function focusTab(page, except) {
  const tabs = await chrome.tabs.query({});
  const open = tabs
    .filter((tab) => tab.id !== except && tab.url && sameTab(tab.url, page))
    .sort((a, b) => (b.lastAccessed ?? 0) - (a.lastAccessed ?? 0))[0];
  if (!open) return false;
  await chrome.tabs.update(open.id, { active: true });
  await chrome.windows.update(open.windowId, { focused: true });
  return true;
}

// The live link: the app asks over a local WebSocket before opening anything, so switching
// to an open tab never shows a new tab. The app pings every 20 seconds, which keeps this
// worker (and the socket) alive; an alarm reconnects after Chrome or the app restarts.
const ZEPHYR = 'ws://127.0.0.1:47391/';
let socket = null;

function connect() {
  if (socket && socket.readyState <= WebSocket.OPEN) return;
  try {
    socket = new WebSocket(ZEPHYR);
  } catch {
    socket = null;
    return;
  }
  socket.onmessage = (event) => void answer(event.data);
  socket.onclose = () => {
    socket = null;
  };
}

async function answer(data) {
  let message;
  try {
    message = JSON.parse(data);
  } catch {
    return;
  }
  if (message.type === 'ping') {
    socket?.send(JSON.stringify({ type: 'pong' }));
  } else if (message.type === 'focus') {
    const found = await focusTab(message.url).catch(() => false);
    socket?.send(JSON.stringify({ type: 'focused', id: message.id, found }));
  } else if (message.type === 'focusPlaying') {
    const found = await focusPlaying(message.title).catch(() => false);
    socket?.send(JSON.stringify({ type: 'focused', id: message.id, found }));
  }
}

/**
 * Brings the tab playing media to the front: an audible one (the one whose title has the
 * track's when several are), or, while the song is paused, the tab whose title has it.
 */
async function focusPlaying(title) {
  const has = (tab) =>
    Boolean(title) && (tab.title ?? '').toLowerCase().includes(title.toLowerCase());
  const latest = (a, b) => (b.lastAccessed ?? 0) - (a.lastAccessed ?? 0);
  const audible = (await chrome.tabs.query({ audible: true })).sort(latest);
  const tab =
    audible.find(has) ?? audible[0] ?? (await chrome.tabs.query({})).sort(latest).find(has);
  if (!tab) return false;
  await chrome.tabs.update(tab.id, { active: true });
  await chrome.windows.update(tab.windowId, { focused: true });
  return true;
}

chrome.runtime.onStartup.addListener(connect);
chrome.runtime.onInstalled.addListener(connect);
chrome.alarms.create('zephyr-link', { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(connect);
connect();

// The fallback, when the link wasn't up: the app opened its hand-off link in a new tab.
chrome.webNavigation.onBeforeNavigate.addListener(async (details) => {
  if (details.frameId !== 0 || !HANDOFF.test(details.url)) return;
  const page = wantedPage(details.url);
  // Nothing open: the hand-off link goes on to the page.
  if (!page || !(await focusTab(page, details.tabId))) return;
  await chrome.tabs.remove(details.tabId).catch(() => undefined);
});
