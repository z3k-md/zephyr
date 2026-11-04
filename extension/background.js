// The app opens bookmarks through its own local hand-off link,
// http://127.0.0.1:<port>/zephyr/open?u=<page>. When a tab is already on that page, this
// switches to it and closes the hand-off tab; otherwise the link redirects to the page as
// it would without the extension.

import { hostBlocked, tabsToBlock } from './focus.js';

const HANDOFF = /^http:\/\/127\.0\.0\.1:\d+\/zephyr\/open\?/;

/** Sites blocked until this unix time; 0 means no focus session is running.
 *  Copied into session storage so a restarted worker still blocks. */
let focusBlock = { sites: [], until: 0 };
/** A message from Zephyr has replaced whatever session storage held. */
let focusFromApp = false;

function blockFrom(message) {
  const sites = Array.isArray(message?.sites)
    ? message.sites.filter((site) => typeof site === 'string')
    : [];
  return { sites, until: Number(message?.until) || 0 };
}

function focusActive() {
  return focusBlock.until * 1000 > Date.now() && focusBlock.sites.length > 0;
}

/** A top-frame navigation to a blocked host, or null. */
function blockedHost(url) {
  if (!focusActive()) return null;
  try {
    const host = new URL(url).hostname;
    return hostBlocked(host, focusBlock.sites) ? host : null;
  } catch {
    return null;
  }
}

/** Opens the blocked page in this tab. */
function redirectToBlocked(tabId, host) {
  const page = new URL(chrome.runtime.getURL('blocked.html'));
  page.searchParams.set('host', host);
  page.searchParams.set('until', String(focusBlock.until));
  return chrome.tabs.update(tabId, { url: page.href }).catch(() => undefined);
}

/** Tabs already open on a blocked host, when a block is applied, restored, or its sites change. */
async function redirectBlockedTabs() {
  try {
    const tabs = await chrome.tabs.query({});
    const chosen = tabsToBlock(tabs, focusBlock, Date.now());
    await Promise.all(chosen.map((tab) => redirectToBlocked(tab.id, tab.host)));
  } catch {
    // A tab can close while we're sending it to the blocked page.
  }
}

/** Restores the block before any navigation is judged. A message that arrived first wins. */
async function restoreFocusBlock() {
  try {
    const stored = await chrome.storage.session.get('focusBlock');
    if (!focusFromApp && stored.focusBlock) {
      focusBlock = blockFrom(stored.focusBlock);
    }
  } catch {
    // Session storage can be missing; the next message from Zephyr fills the block in.
  }
  if (!focusFromApp) await redirectBlockedTabs();
}

const focusReady = restoreFocusBlock();

async function applyFocusBlock(block) {
  focusFromApp = true;
  focusBlock = block;
  try {
    await chrome.storage.session.set({ focusBlock: block });
  } catch {
    // The in-memory block still applies until this worker stops.
  }
  await redirectBlockedTabs();
}

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
  } else if (message.type === 'focusBlock') {
    await applyFocusBlock(blockFrom(message));
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

// While a focus session runs, a top-frame navigation to a listed site (or a subdomain of
// it) opens the blocked page instead. The extension page itself isn't one of those hosts.
// The saved block is restored before this decides, so a restarted worker still blocks.
chrome.webNavigation.onBeforeNavigate.addListener(async (details) => {
  await focusReady;
  if (details.frameId !== 0) return;
  const host = blockedHost(details.url);
  if (!host) return;
  await redirectToBlocked(details.tabId, host);
});

// The fallback, when the link wasn't up: the app opened its hand-off link in a new tab.
chrome.webNavigation.onBeforeNavigate.addListener(async (details) => {
  if (details.frameId !== 0 || !HANDOFF.test(details.url)) return;
  const page = wantedPage(details.url);
  // Nothing open: the hand-off link goes on to the page.
  if (!page || !(await focusTab(page, details.tabId))) return;
  await chrome.tabs.remove(details.tabId).catch(() => undefined);
});
