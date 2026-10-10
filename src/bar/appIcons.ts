import { reactive } from 'vue';
import { appIcon } from '../api';

// Real app icons for rows, fetched once per app and kept for the session. They arrive after
// the rows draw, so typing never waits; null means the app has none to offer.
const icons = reactive<Record<string, string | null>>({});

export function iconFor(appId: string, size: number): string | null {
  if (!(appId in icons)) {
    icons[appId] = null;
    const pixels = Math.round(size * (window.devicePixelRatio || 1));
    void appIcon(appId, pixels)
      .then((url) => (icons[appId] = url))
      .catch(() => undefined);
  }
  return icons[appId];
}
