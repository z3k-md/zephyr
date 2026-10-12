import { reactive } from 'vue';
import { appIcon, siteIcon, siteIconUrl } from '../api';

// Real icons for rows: an installed app's own icon, and a web destination's site icon. Each is
// fetched once and kept for the session; they arrive after the rows draw, so typing never
// waits. null means there is none (the row keeps its letter tile).
const icons = reactive<Record<string, string | null>>({});

export function iconFor(appId: string, size: number): string | null {
  const key = `app:${appId}`;
  if (!(key in icons)) {
    icons[key] = null;
    const pixels = Math.round(size * (window.devicePixelRatio || 1));
    void appIcon(appId, pixels)
      .then((url) => (icons[key] = url))
      .catch(() => undefined);
  }
  return icons[key];
}

export function siteIconFor(destinationId: string): string | null {
  const key = `dest:${destinationId}`;
  if (!(key in icons)) {
    icons[key] = null;
    void siteIcon(destinationId)
      .then((url) => (icons[key] = url))
      .catch(() => undefined);
  }
  return icons[key];
}

/** A site's icon by address (bookmarks), shared per site. */
export function siteIconForUrl(url: string): string | null {
  let host = url;
  try {
    host = new URL(url).host;
  } catch {
    return null;
  }
  const key = `site:${host}`;
  if (!(key in icons)) {
    icons[key] = null;
    void siteIconUrl(url)
      .then((data) => (icons[key] = data))
      .catch(() => undefined);
  }
  return icons[key];
}
