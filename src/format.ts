// Numbers, dates and names as the windows show them.

/** A size in binary units, as Windows shows it: "4.2 GB", "512 MB" (1 KB is 1024 bytes). */
export function bytes(value: number, digits = 1): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : value < 10 ? digits : 0)} ${units[unit]}`;
}

/** Unix seconds as a local date and time: "Oct 23, 2026, 4:05 PM". */
export function dateTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
}

/** A synced device's platform id as people say it. */
export const PLATFORM_NAMES: Record<string, string> = {
  macos: 'Mac',
  windows: 'Windows',
  linux: 'Linux',
};
