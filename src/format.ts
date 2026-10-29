// Numbers, dates and names as the windows show them.

import { ref, watch } from 'vue';
import { barShown, pageShown } from './visibility';

/**
 * The time (ms), moved on every 10 seconds while the window can be seen. Text that says how
 * long ago something was reads it, so a template or computed showing "2m" keeps up.
 */
export const clock = ref(Date.now());
if (typeof window !== 'undefined') {
  window.setInterval(() => {
    if (barShown.value && pageShown.value) clock.value = Date.now();
  }, 10_000);
  // Seen again: catch up at once rather than at the next tick.
  watch([barShown, pageShown], ([bar, page]) => {
    if (bar && page) clock.value = Date.now();
  });
}

/** The time now, tracked through the clock so whatever reads it updates with it. */
function now(): number {
  return Math.max(clock.value, Date.now());
}

/** A stretch of time: "40 s", "12 min", "3 h". */
export function elapsed(seconds: number): string {
  const whole = Math.max(0, Math.round(seconds));
  if (whole < 90) return `${whole} s`;
  if (whole < 90 * 60) return `${Math.round(whole / 60)} min`;
  return `${Math.round(whole / 3600)} h`;
}

/**
 * How long ago a Unix time (seconds) was, in one of three styles:
 * - `words`: "40 s ago", "12 min ago", "3 h ago"
 * - `short`, for a list's corner: "now", "2m", "3h", "4d"
 * - `recent`: "just now", "2m ago", "3h ago", then the date
 */
export function ago(
  at: number,
  style: 'words' | 'short' | 'recent' = 'words',
  nowMs: number = now()
): string {
  const diff = Math.max(0, nowMs / 1000 - at);
  if (style === 'words') return `${elapsed(diff)} ago`;
  if (diff < 60) return style === 'short' ? 'now' : 'just now';
  const suffix = style === 'short' ? '' : ' ago';
  if (diff < 3600) return `${Math.floor(diff / 60)}m${suffix}`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h${suffix}`;
  if (style === 'short') return `${Math.floor(diff / 86400)}d`;
  return new Date(at * 1000).toLocaleDateString();
}

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
