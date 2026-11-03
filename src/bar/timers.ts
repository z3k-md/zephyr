// Reading "timer 120", "timer 5m", "timer 1h30m tea" and "timer 2:30" into a length and a
// name, and how a running timer's time left reads. Pure, so it's tested on its own.

/** The longest timer the app takes (countdown.rs MAX_SECONDS): a week. */
export const MAX_TIMER_SECONDS = 7 * 24 * 3600;

/** The lengths offered under a bare "timer". */
export const TIMER_PRESETS = [300, 600, 900];

export interface TimerAsk {
  /** None for a bare "timer", which offers the presets. */
  seconds: number | null;
  label: string;
}

const UNITS: Record<string, number> = {
  h: 3600,
  hr: 3600,
  hrs: 3600,
  hour: 3600,
  hours: 3600,
  m: 60,
  min: 60,
  mins: 60,
  minute: 60,
  minutes: 60,
  s: 1,
  sec: 1,
  secs: 1,
  second: 1,
  seconds: 1,
};

/** "120" (seconds), "5m", "1h 30m", "1.5h", "90 sec", "2:30", "1:02:03" → seconds. */
export function parseDuration(text: string): number | null {
  const trimmed = text.trim().toLowerCase();
  if (!trimmed) return null;
  if (/^\d+$/.test(trimmed)) return Number(trimmed);
  if (/^\d+(?::[0-5]?\d){1,2}$/.test(trimmed)) {
    return trimmed.split(':').reduce((total, part) => total * 60 + Number(part), 0);
  }
  const part = /(\d+(?:\.\d+)?)\s*([a-z]+)[\s,]*/y;
  let total = 0;
  let matched = 0;
  part.lastIndex = 0;
  while (part.lastIndex < trimmed.length) {
    const match = part.exec(trimmed);
    if (!match) return null;
    const scale = UNITS[match[2]];
    if (!scale) return null;
    total += Number(match[1]) * scale;
    matched++;
  }
  return matched ? Math.round(total) : null;
}

/** What "timer …" asks for, or null when the text isn't a timer command. */
export function parseTimer(typed: string): TimerAsk | null {
  const match = /^\s*timer(?:\s+(.*))?$/i.exec(typed);
  if (!match) return null;
  const rest = (match[1] ?? '').trim();
  if (!rest) return { seconds: null, label: '' };
  // The longest run of words that reads as a length; the words after it name the timer.
  const words = rest.split(/\s+/);
  for (let end = words.length; end > 0; end--) {
    const seconds = parseDuration(words.slice(0, end).join(' '));
    if (seconds !== null) {
      if (seconds < 1 || seconds > MAX_TIMER_SECONDS) return null;
      return { seconds, label: words.slice(end).join(' ') };
    }
  }
  return null;
}

/** A length in words: "45 seconds", "5 minutes", "1h 30m", "2m 15s". */
export function timerLength(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  if (!hours && !secs) return minutes === 1 ? '1 minute' : `${minutes} minutes`;
  if (!hours && !minutes) return secs === 1 ? '1 second' : `${secs} seconds`;
  return [hours && `${hours}h`, minutes && `${minutes}m`, secs && `${secs}s`]
    .filter(Boolean)
    .join(' ');
}

/** Time left on a clock face: "4:05", "1:02:09". */
export function timeLeft(seconds: number): string {
  const left = Math.max(0, Math.ceil(seconds));
  const hours = Math.floor(left / 3600);
  const minutes = Math.floor((left % 3600) / 60);
  const secs = String(left % 60).padStart(2, '0');
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${secs}` : `${minutes}:${secs}`;
}
