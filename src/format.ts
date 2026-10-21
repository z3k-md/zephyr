// Numbers as the bar shows them.

/** A size in decimal units, as Activity Monitor and Finder show it: "4.2 GB", "512 MB". */
export function bytes(value: number, digits = 1): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : value < 10 ? digits : 0)} ${units[unit]}`;
}
