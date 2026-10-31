import { invoke } from '@tauri-apps/api/core';

// Every window's warnings, errors, uncaught exceptions and failed commands go to Zephyr's log
// file (logger.rs), in release builds too, so a bug shows up there without devtools open.

type Level = 'info' | 'warn' | 'error';

/** The same line again within this long is dropped, so a render loop can't flood the log. */
const REPEAT_MS = 60_000;
/** At most this many lines a minute from one window; the rest are counted, not written. */
const PER_MINUTE = 30;
const MINUTE_MS = 60_000;
const MAX_REMEMBERED = 200;

export function describe(part: unknown): string {
  if (part instanceof Error) return part.stack || String(part);
  if (typeof part === 'string') return part;
  try {
    return JSON.stringify(part) ?? String(part);
  } catch {
    return String(part);
  }
}

export class Limiter {
  private seen = new Map<string, number>();
  private minuteStart = -Infinity;
  private sent = 0;
  private dropped = 0;

  /**
   * Whether `text` should be written at `now`. Numbers don't count, so lines that differ only
   * by a count, a pid or a time are the same line.
   */
  allow(text: string, now: number): boolean {
    const key = text.replace(/\d+/g, '#');
    const last = this.seen.get(key);
    if (last !== undefined && now - last < REPEAT_MS) return false;
    if (now - this.minuteStart >= MINUTE_MS) {
      this.minuteStart = now;
      this.sent = 0;
    }
    if (this.sent >= PER_MINUTE) {
      this.dropped += 1;
      return false;
    }
    this.sent += 1;
    this.seen.delete(key);
    if (this.seen.size >= MAX_REMEMBERED) this.seen.delete(this.seen.keys().next().value!);
    this.seen.set(key, now);
    return true;
  }

  /** How many lines the cap has dropped since this was last asked. */
  takeDropped(): number {
    const dropped = this.dropped;
    this.dropped = 0;
    return dropped;
  }
}

const limiter = new Limiter();

function send(level: Level, text: string): void {
  const page = location.pathname + location.search;
  // Not through api.ts: a failure here would log itself again.
  void invoke('log_webview', { level, page, text }).catch(() => undefined);
}

export function logToFile(level: Level, text: string): void {
  if (!limiter.allow(text, Date.now())) return;
  const dropped = limiter.takeDropped();
  if (dropped) send('warn', `${dropped} more lines were dropped: over ${PER_MINUTE} a minute`);
  send(level, text);
}

/**
 * A command that failed, with the names of its arguments but not their values, which can be
 * clipboard text, notes or secrets.
 */
export function logCommandFailure(command: string, args: unknown, error: unknown): void {
  const keys = args && typeof args === 'object' ? Object.keys(args).join(', ') : '';
  logToFile('warn', `command ${command}(${keys}) failed: ${describe(error)}`);
}

let installed = false;

export function captureConsole(): void {
  if (installed) return;
  installed = true;
  for (const level of ['warn', 'error'] as const) {
    const original = console[level].bind(console);
    console[level] = (...parts: unknown[]) => {
      original(...parts);
      logToFile(level, parts.map(describe).join(' '));
    };
  }
  addEventListener('error', (event) => {
    logToFile('error', `uncaught: ${describe(event.error ?? event.message)}`);
  });
  addEventListener('unhandledrejection', (event) => {
    logToFile('error', `unhandled rejection: ${describe(event.reason)}`);
  });
}
