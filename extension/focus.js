/** Whether `hostname` is a listed site or a subdomain of one.
 *  `youtube.com` blocks `m.youtube.com`, not `notyoutube.com`. */
export function hostBlocked(hostname, sites) {
  const host = String(hostname ?? '')
    .toLowerCase()
    .replace(/\.$/, '');
  if (!host) return false;
  return sites.some((site) => {
    const root = String(site ?? '')
      .trim()
      .toLowerCase()
      .replace(/^\.+/, '')
      .replace(/\.$/, '');
    return root !== '' && (host === root || host.endsWith(`.${root}`));
  });
}

/** Tabs to send to the blocked page: those whose host is listed.
 *  A tab with no address, or a host that isn't listed, is left out.
 *  `now` is milliseconds; a block that has ended selects nothing. */
export function tabsToBlock(tabs, block, now) {
  const sites = Array.isArray(block?.sites) ? block.sites : [];
  const until = Number(block?.until) || 0;
  if (until * 1000 <= now || sites.length === 0) return [];
  const chosen = [];
  for (const tab of tabs ?? []) {
    if (tab?.id == null || !tab.url) continue;
    let host;
    try {
      host = new URL(tab.url).hostname;
    } catch {
      continue;
    }
    if (hostBlocked(host, sites)) chosen.push({ id: tab.id, host });
  }
  return chosen;
}
