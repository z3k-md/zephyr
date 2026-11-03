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
