// The agenda on the bar: `cal` lists the day, a meeting about to start leads the empty
// bar, and the footer names the next one. Rust fetches the feeds.

import { onMounted, onUnmounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { calendarAgenda } from '../api';
import type { Agenda } from '../types';
import { barShown } from '../visibility';
import {
  agendaLeads,
  agendaRows,
  calendarDay,
  footerChip,
  soonRow,
  type CalendarDay,
} from './calendar';

export function useCalendar() {
  const agenda = ref<Agenda | null>(null);
  const now = ref(Date.now());
  let unlisten: UnlistenFn | null = null;
  let tick = 0;

  onMounted(async () => {
    agenda.value = await calendarAgenda().catch(() => null);
    unlisten = await listen<Agenda>('calendar', (event) => {
      agenda.value = event.payload;
    });
  });
  onUnmounted(() => {
    unlisten?.();
    window.clearInterval(tick);
  });

  watch(
    () => barShown.value && (agenda.value?.events.length ?? 0) > 0,
    (ticking) => {
      window.clearInterval(tick);
      tick = 0;
      if (!ticking) return;
      now.value = Date.now();
      tick = window.setInterval(() => (now.value = Date.now()), 1000);
    },
    { immediate: true }
  );

  /** The agenda for the typed text, and whether it leads the list. None until a feed is
   *  added, so `cal` and `calendar` stay ordinary searches. */
  function rowsFor(text: string): {
    rows: ReturnType<typeof agendaRows>['rows'];
    preselect: number | null;
    leads: boolean;
  } | null {
    const day: CalendarDay | null = calendarDay(text);
    if (!day || !agenda.value?.feeds.length) return null;
    return { ...agendaRows(agenda.value, day, now.value), leads: agendaLeads(text) };
  }

  function soon() {
    if (!agenda.value?.events.length) return null;
    return soonRow(agenda.value.events, now.value);
  }

  function chip() {
    if (!agenda.value?.events.length) return null;
    return footerChip(agenda.value.events, now.value);
  }

  return { rowsFor, soon, chip, now };
}
