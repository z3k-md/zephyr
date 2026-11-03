import { describe, expect, it } from 'vitest';
import type { CalEvent } from '../types';
import {
  agendaRows,
  calendarDay,
  enterAction,
  footerChip,
  lengthLabel,
  soonRow,
  subtitle,
} from './calendar';

function event(extra: Partial<CalEvent> = {}): CalEvent {
  const start = new Date(2026, 10, 3, 9, 0, 0, 0).getTime();
  return {
    id: 'e1',
    feedId: 'f',
    color: '#5b8def',
    title: 'Design review',
    startMs: start,
    endMs: start + 45 * 60_000,
    allDay: false,
    startDate: '',
    endDate: '',
    location: 'Room 4',
    url: 'https://meet.google.com/abc-defg-hij',
    provider: 'Google Meet',
    ...extra,
  };
}

describe('calendarDay', () => {
  it('takes cal, calendar and agenda, and tomorrow', () => {
    expect(calendarDay('cal')).toBe('today');
    expect(calendarDay('calendar')).toBe('today');
    expect(calendarDay('agenda')).toBe('today');
    expect(calendarDay('cal tomorrow')).toBe('tomorrow');
    expect(calendarDay('cale')).toBe('today');
    expect(calendarDay('agen')).toBe('today');
    expect(calendarDay('cal tom')).toBe('tomorrow');
    expect(calendarDay('cal tod')).toBe('today');
  });

  it('leaves nearby words as searches', () => {
    expect(calendarDay('call')).toBeNull();
    expect(calendarDay('age')).toBeNull();
    expect(calendarDay('cal next week')).toBeNull();
    expect(calendarDay('')).toBeNull();
  });
});

describe('rows', () => {
  const now = new Date(2026, 10, 3, 8, 50, 0, 0).getTime();

  it('puts all-day events first and dims ones that are over', () => {
    const timed = event();
    const over = event({
      id: 'over',
      title: 'Standup',
      startMs: now - 2 * 60 * 60_000,
      endMs: now - 60 * 60_000,
      url: '',
      provider: '',
      location: '',
    });
    const allDay = event({
      id: 'day',
      title: 'Offsite',
      allDay: true,
      startDate: '2026-11-03',
      endDate: '2026-11-04',
      url: '',
      provider: '',
      location: '',
    });
    const { rows, preselect } = agendaRows(
      { events: [over, timed, allDay], feeds: [] },
      'today',
      now
    );
    expect(rows.map((row) => row.label)).toEqual(['Offsite', 'Standup', 'Design review']);
    expect(rows[0].calendar?.time).toBe('All day');
    expect(rows[1].calendar?.past).toBe(true);
    expect(rows[2].calendar?.past).toBe(false);
    expect(preselect).toBe(0);
    expect(rows[2].subtitle).toBe('45 min · Google Meet · Room 4');
    expect(rows[2].hint).toBe('Join');
  });

  it('drops a location that is only the link', () => {
    const meet = event({ location: 'https://meet.google.com/abc-defg-hij' });
    expect(subtitle(meet)).toBe('45 min · Google Meet');
    expect(lengthLabel(event({ endMs: event().startMs + 90 * 60_000 }))).toBe('1 hr 30 min');
  });

  it('leads with a meeting inside the soon window, and joins only with a link', () => {
    const soon = event({ startMs: now + 4 * 60_000, endMs: now + 50 * 60_000 });
    const later = event({
      id: 'later',
      title: 'Later',
      startMs: now + 30 * 60_000,
      endMs: now + 90 * 60_000,
    });
    const row = soonRow([later, soon], now);
    expect(row?.label).toBe('Design review · in 4 min');
    expect(row?.kind).toBe('calendarSoon');
    expect(enterAction(row!)).toBe('join');
    const quiet = soonRow(
      [event({ url: '', startMs: now + 4 * 60_000, endMs: now + 50 * 60_000 })],
      now
    );
    expect(enterAction(quiet!)).toBe('none');
    expect(soonRow([later], now)).toBeNull();
  });

  it('names the next meeting in the footer within 15 minutes', () => {
    const meeting = event({ startMs: now + 12 * 60_000, endMs: now + 40 * 60_000 });
    expect(footerChip([meeting], now)).toEqual({ title: 'Design review', when: 'in 12m' });
    expect(
      footerChip([event({ startMs: now + 20 * 60_000, endMs: now + 80 * 60_000 })], now)
    ).toBeNull();
    expect(
      footerChip([event({ startMs: now - 2 * 60_000, endMs: now + 30 * 60_000 })], now)
    ).toEqual({
      title: 'Design review',
      when: 'now',
    });
  });

  it('offers Settings when no feed is saved, and nothing on an empty day', () => {
    const empty = agendaRows({ events: [], feeds: [] }, 'today', now);
    expect(empty.rows[0].label).toBe('Add a calendar in Settings');
    expect(enterAction(empty.rows[0])).toBe('settings');
    const quiet = agendaRows(
      {
        events: [],
        feeds: [{ id: 'f', name: 'Work', color: '#5b8def', order: 0, updatedAt: 1, error: '' }],
      },
      'tomorrow',
      now
    );
    expect(quiet.rows[0].label).toBe('Nothing tomorrow');
    expect(enterAction(quiet.rows[0])).toBe('none');
  });
});
