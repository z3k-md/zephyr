// Agenda rows for `cal`, `calendar` and `agenda`. Rust keeps the events; this decides
// which day, how a row reads, and what Enter does.

import type { Agenda, CalEvent, CalendarRow, SuggestionRow } from '../types';

export type CalendarDay = 'today' | 'tomorrow';

const SOON_BEFORE_MS = 10 * 60 * 1000;
const SOON_AFTER_MS = 5 * 60 * 1000;
const FOOTER_MS = 15 * 60 * 1000;

/** `cal`, `calendar` or `agenda`, and `today` or `tomorrow` while those are being typed.
 *  `call` and `age` stay ordinary searches. */
export function calendarDay(text: string): CalendarDay | null {
  const typed = text.trim().toLowerCase().replace(/\s+/g, ' ');
  if (!typed) return null;
  const space = typed.indexOf(' ');
  const head = space < 0 ? typed : typed.slice(0, space);
  const tail = space < 0 ? '' : typed.slice(space + 1);
  const named =
    head === 'cal' ||
    (head.length >= 4 && ('calendar'.startsWith(head) || 'agenda'.startsWith(head)));
  if (!named) return null;
  if (!tail) return 'today';
  const today = 'today'.startsWith(tail);
  const tomorrow = 'tomorrow'.startsWith(tail);
  if (tomorrow && !today) return 'tomorrow';
  if (today) return 'today';
  return null;
}

/** Whether the agenda leads the list instead of following the other results. `cal` is how
 *  Calculator is reached and `calendar` names the Calendar app, so only `agenda` in full or
 *  a named day (`cal today`, `cal tomorrow`) put it first. */
export function agendaLeads(text: string): boolean {
  if (!calendarDay(text)) return false;
  const typed = text.trim().toLowerCase().replace(/\s+/g, ' ');
  return typed === 'agenda' || typed.includes(' ');
}

export function ymd(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${date.getFullYear()}-${month}-${day}`;
}

function dayAt(now: number, offset: number): Date {
  const date = new Date(now);
  date.setHours(0, 0, 0, 0);
  date.setDate(date.getDate() + offset);
  return date;
}

export function onDay(event: CalEvent, day: Date): boolean {
  const start = day.getTime();
  const end = start + 86_400_000;
  if (event.allDay) {
    const key = ymd(day);
    return event.startDate <= key && event.endDate > key;
  }
  if (event.endMs <= event.startMs) return event.startMs >= start && event.startMs < end;
  return event.startMs < end && event.endMs > start;
}

export function lengthLabel(event: CalEvent): string {
  if (event.allDay) {
    const days = daySpan(event.startDate, event.endDate);
    if (days <= 1) return 'all day';
    return `${days} days`;
  }
  const mins = Math.round((event.endMs - event.startMs) / 60_000);
  if (mins <= 0) return '';
  if (mins < 60) return mins === 1 ? '1 min' : `${mins} min`;
  const hours = Math.floor(mins / 60);
  const rest = mins % 60;
  const hour = hours === 1 ? '1 hr' : `${hours} hr`;
  return rest === 0 ? hour : `${hour} ${rest} min`;
}

function daySpan(start: string, end: string): number {
  const from = Date.parse(`${start}T00:00:00`);
  const to = Date.parse(`${end}T00:00:00`);
  if (!Number.isFinite(from) || !Number.isFinite(to) || to <= from) return 1;
  return Math.round((to - from) / 86_400_000);
}

function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
}

/** The place, unless it is only the meeting link. */
function place(event: CalEvent): string {
  const location = event.location.trim();
  if (!location || location === event.url) return '';
  if (/^https?:\/\//i.test(location)) return '';
  return location;
}

export function subtitle(event: CalEvent): string {
  return [lengthLabel(event), event.provider, place(event)].filter(Boolean).join(' · ');
}

function whenLabel(event: CalEvent): string {
  if (event.allDay) {
    const start = dateOnly(event.startDate);
    const last = dateOnly(event.endDate);
    last.setDate(last.getDate() - 1);
    const fmt = (date: Date) =>
      date.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
    if (ymd(start) === ymd(last)) return fmt(start);
    return `${fmt(start)} – ${fmt(last)}`;
  }
  const start = new Date(event.startMs);
  const day = start.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
  if (event.endMs <= event.startMs) return `${day}, ${clock(event.startMs)}`;
  return `${day}, ${clock(event.startMs)}–${clock(event.endMs)}`;
}

function dateOnly(key: string): Date {
  const [year, month, day] = key.split('-').map((part) => Number(part));
  return new Date(year || 1970, (month || 1) - 1, day || 1);
}

export function detailsText(event: CalEvent): string {
  const lines = [event.title, whenLabel(event)];
  const sub = subtitle(event);
  if (sub) lines.push(sub);
  if (event.url) lines.push(event.url);
  return lines.join('\n');
}

function past(event: CalEvent, now: number): boolean {
  if (event.allDay) return event.endDate <= ymd(new Date(now));
  return event.endMs <= now;
}

function compare(a: CalEvent, b: CalEvent): number {
  if (a.allDay !== b.allDay) return a.allDay ? -1 : 1;
  if (a.allDay) return a.startDate.localeCompare(b.startDate) || a.title.localeCompare(b.title);
  return a.startMs - b.startMs || a.title.localeCompare(b.title);
}

function rowFor(event: CalEvent, now: number, soon = false): SuggestionRow {
  const over = past(event, now);
  const calendar: CalendarRow = {
    id: event.id,
    color: event.color || '#5b8def',
    time: soon ? '' : event.allDay ? 'All day' : clock(event.startMs),
    url: event.url,
    details: detailsText(event),
    past: over,
  };
  if (soon) {
    return {
      kind: 'calendarSoon',
      label: `${event.title} · ${relative(event.startMs, now)}`,
      query: '',
      target: '',
      hint: event.url ? 'Join' : '',
      section: '',
      calendar,
    };
  }
  return {
    kind: 'calendar',
    label: event.title,
    query: '',
    target: '',
    hint: event.url ? 'Join' : 'Copy',
    section: '',
    subtitle: subtitle(event),
    calendar,
  };
}

function relative(startMs: number, now: number): string {
  const delta = startMs - now;
  if (delta <= 30_000) return 'now';
  const mins = Math.max(1, Math.round(delta / 60_000));
  return `in ${mins} min`;
}

/** Today's or tomorrow's rows, or a single line when there's nothing to show. */
export function agendaRows(
  agenda: Agenda | null,
  day: CalendarDay,
  now: number
): { rows: SuggestionRow[]; preselect: number | null } {
  const section = day === 'tomorrow' ? 'tomorrow' : 'today';
  const feeds = agenda?.feeds.length ?? 0;
  const events = (agenda?.events ?? [])
    .filter((event) => onDay(event, dayAt(now, day === 'tomorrow' ? 1 : 0)))
    .sort(compare);
  if (!events.length) {
    const empty = feeds === 0;
    return {
      rows: [
        {
          kind: 'calendar',
          label: empty
            ? 'Add a calendar in Settings'
            : day === 'tomorrow'
              ? 'Nothing tomorrow'
              : 'Nothing today',
          query: '',
          target: '',
          hint: empty ? 'Settings' : '',
          section,
          calendar: {
            id: empty ? 'setup' : `none-${day}`,
            color: '#5b8def',
            time: '',
            url: '',
            details: '',
            past: false,
          },
        },
      ],
      preselect: 0,
    };
  }
  const rows = events.map((event) => {
    const row = rowFor(event, now);
    row.section = section;
    return row;
  });
  const next = rows.findIndex((row) => !row.calendar?.past);
  return { rows, preselect: next >= 0 ? next : 0 };
}

/** A meeting starting within 10 minutes, or one that started less than 5 minutes ago. */
export function soonEvent(events: CalEvent[], now: number): CalEvent | null {
  const hits = events
    .filter((event) => !event.allDay && event.endMs > now)
    .filter(
      (event) => event.startMs >= now - SOON_AFTER_MS && event.startMs <= now + SOON_BEFORE_MS
    )
    .sort((a, b) => a.startMs - b.startMs);
  return hits[0] ?? null;
}

export function soonRow(events: CalEvent[], now: number): SuggestionRow | null {
  const event = soonEvent(events, now);
  return event ? rowFor(event, now, true) : null;
}

/** `Design review in 12m` when the next meeting is within 15 minutes. */
export function footerChip(
  events: CalEvent[],
  now: number
): { title: string; when: string } | null {
  const hits = events
    .filter((event) => !event.allDay && event.endMs > now)
    .filter((event) => {
      const delta = event.startMs - now;
      return delta <= FOOTER_MS && delta >= -FOOTER_MS;
    })
    .sort((a, b) => a.startMs - b.startMs);
  const event = hits[0];
  if (!event) return null;
  const delta = event.startMs - now;
  const when = delta <= 30_000 ? 'now' : `in ${Math.max(1, Math.round(delta / 60_000))}m`;
  return { title: event.title, when };
}

/** What Enter does with a calendar row. */
export function enterAction(row: SuggestionRow): 'join' | 'copy' | 'settings' | 'none' {
  const calendar = row.calendar;
  if (!calendar || (row.kind !== 'calendar' && row.kind !== 'calendarSoon')) return 'none';
  if (calendar.url) return 'join';
  if (row.kind === 'calendarSoon') return 'none';
  if (calendar.id === 'setup') return 'settings';
  if (calendar.details) return 'copy';
  return 'none';
}
