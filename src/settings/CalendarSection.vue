<script setup lang="ts">
  import { computed, onMounted, onUnmounted, ref } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    calendarAddFeed,
    calendarAgenda,
    calendarMoveFeed,
    calendarRemoveFeed,
    calendarRenameFeed,
    errorMessage,
  } from '../api';
  import type { Agenda, CalendarFeed, Snapshot } from '../types';

  // Settings › Calendar: named iCal feeds. The link is a secret, so it goes to the keychain
  // and is not shown again. The name, color and order stay in settings.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  const agenda = ref<Agenda | null>(null);
  const name = ref('');
  const link = ref('');
  const busy = ref(false);
  const error = ref<string | null>(null);
  const editing = ref<string | null>(null);
  const editName = ref('');
  let unlisten: UnlistenFn | null = null;

  const feeds = computed(() =>
    [...(props.snapshot.calendarFeeds ?? [])].sort(
      (a, b) => a.order - b.order || a.name.localeCompare(b.name)
    )
  );

  onMounted(async () => {
    agenda.value = await calendarAgenda().catch(() => null);
    unlisten = await listen<Agenda>('calendar', (event) => {
      agenda.value = event.payload;
    });
  });
  onUnmounted(() => unlisten?.());

  function status(feed: CalendarFeed): string {
    const live = agenda.value?.feeds.find((item) => item.id === feed.id);
    if (!live || !live.updatedAt) return live?.error || 'Updating…';
    if (live.error) return live.error;
    const when = new Date(live.updatedAt);
    const time = when.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    const same = when.toDateString() === new Date().toDateString();
    return same
      ? `Updated ${time}`
      : `Updated ${when.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${time}`;
  }

  async function save(request: Promise<Snapshot>) {
    busy.value = true;
    try {
      emit('apply', await request);
      error.value = null;
    } catch (err) {
      error.value = errorMessage(err);
    } finally {
      busy.value = false;
    }
  }

  function add() {
    const title = name.value.trim();
    const url = link.value.trim();
    if (!title || !url) return;
    void save(calendarAddFeed(title, url)).then(() => {
      if (!error.value) {
        name.value = '';
        link.value = '';
      }
    });
  }

  function rename(feed: CalendarFeed) {
    const title = editName.value.trim();
    editing.value = null;
    if (!title || title === feed.name) return;
    void save(calendarRenameFeed(feed.id, title));
  }
</script>

<template>
  <section id="section-calendar">
    <h2>Calendar</h2>
    <p class="hint-text">
      Google Calendar: calendar settings, Integrate calendar, Secret address in iCal format.
      Outlook: publish the calendar and copy the ICS link. iCloud: public calendar, Share Calendar.
    </p>
    <form class="field inline" data-focus="feed" @submit.prevent="add">
      <input
        v-model="name"
        type="text"
        placeholder="Name"
        spellcheck="false"
        aria-label="Calendar name"
      />
      <input
        v-model="link"
        type="url"
        placeholder="iCal link"
        spellcheck="false"
        autocomplete="off"
        aria-label="iCal link"
      />
      <button type="submit" :disabled="busy || !name.trim() || !link.trim()">Add</button>
    </form>
    <ul v-if="feeds.length" class="folders cal-feeds">
      <li v-for="(feed, index) in feeds" :key="feed.id">
        <span class="swatch" :style="{ background: feed.color }" aria-hidden="true" />
        <span class="cal-feed">
          <template v-if="editing === feed.id">
            <input
              v-model="editName"
              type="text"
              spellcheck="false"
              aria-label="Calendar name"
              @keydown.enter.prevent="rename(feed)"
              @keydown.escape.prevent="editing = null"
            />
          </template>
          <template v-else>
            <b>{{ feed.name }}</b>
            <span class="hint-text">{{ status(feed) }}</span>
          </template>
        </span>
        <span class="cal-feed-actions">
          <button
            type="button"
            :disabled="busy || index === 0"
            @click="save(calendarMoveFeed(feed.id, true))"
          >
            Up
          </button>
          <button
            type="button"
            :disabled="busy || index === feeds.length - 1"
            @click="save(calendarMoveFeed(feed.id, false))"
          >
            Down
          </button>
          <button
            v-if="editing !== feed.id"
            type="button"
            :disabled="busy"
            @click="
              editing = feed.id;
              editName = feed.name;
            "
          >
            Rename
          </button>
          <button v-else type="button" :disabled="busy" @click="rename(feed)">Save</button>
          <button type="button" :disabled="busy" @click="save(calendarRemoveFeed(feed.id))">
            Remove
          </button>
        </span>
      </li>
    </ul>
    <p v-if="error" class="notice">{{ error }}</p>
  </section>
</template>
