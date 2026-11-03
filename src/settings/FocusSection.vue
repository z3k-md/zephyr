<script setup lang="ts">
  import { computed, ref } from 'vue';
  import { errorMessage, saveFocus } from '../api';
  import RowIcon from '../bar/RowIcon.vue';
  import InfoTip from '../components/InfoTip.vue';
  import type { FocusApp, FocusSettings, Snapshot } from '../types';
  import AppPicker from './AppPicker.vue';

  // Settings › Focus: how long "focus deep work" runs, the break that starts when a session
  // ends, apps whose windows are minimized or hidden, and sites the browser extension blocks.
  // A break of 0 starts none. Without the extension, a session still runs.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  const WEEK_MINUTES = 7 * 24 * 60;
  const FALLBACK: FocusSettings = {
    seconds: 25 * 60,
    breakSeconds: 5 * 60,
    apps: [],
    sites: [],
  };

  const error = ref<string | null>(null);
  const busy = ref(false);
  const siteText = ref('');
  let draft: FocusSettings | null = null;
  let saving: Promise<void> = Promise.resolve();

  const settings = computed(() => props.snapshot.focus ?? FALLBACK);
  const lengthMinutes = computed(() => Math.round((draft ?? settings.value).seconds / 60));
  const breakMinutes = computed(() => Math.round((draft ?? settings.value).breakSeconds / 60));
  const apps = computed(() => (draft ?? settings.value).apps);
  const sites = computed(() => (draft ?? settings.value).sites);

  function current(): FocusSettings {
    return draft ?? settings.value;
  }

  function save(next: FocusSettings) {
    draft = next;
    busy.value = true;
    error.value = null;
    saving = saving
      .then(async () => emit('apply', await saveFocus(next)))
      .catch((err) => {
        error.value = errorMessage(err);
      })
      .finally(() => {
        if (draft === next) {
          draft = null;
          busy.value = false;
        }
      });
  }

  function minutesOf(input: HTMLInputElement, allowZero: boolean): number | null {
    const minutes = Number(input.value);
    const least = allowZero ? 0 : 1;
    if (!Number.isInteger(minutes) || minutes < least || minutes > WEEK_MINUTES) {
      input.value = String(allowZero ? breakMinutes.value : lengthMinutes.value);
      return null;
    }
    return minutes;
  }

  function onLength(event: Event) {
    const input = event.target as HTMLInputElement;
    const minutes = minutesOf(input, false);
    if (minutes === null) return;
    save({ ...current(), seconds: minutes * 60 });
  }

  function onBreak(event: Event) {
    const input = event.target as HTMLInputElement;
    const minutes = minutesOf(input, true);
    if (minutes === null) return;
    save({ ...current(), breakSeconds: minutes * 60 });
  }

  function addApp(id: string, name: string) {
    const appsNext: FocusApp[] = [...current().apps, { id, name }];
    save({ ...current(), apps: appsNext });
  }

  function removeApp(id: string) {
    save({ ...current(), apps: current().apps.filter((app) => app.id !== id) });
  }

  /** "https://www.YouTube.com/feed" is "youtube.com". The same rule as focus.rs. */
  function hostOf(text: string): string | null {
    const trimmed = text.trim().toLowerCase();
    if (!trimmed || /\s/.test(trimmed)) return null;
    try {
      const url = new URL(trimmed.includes('://') ? trimmed : `https://${trimmed}`);
      let host = url.hostname.replace(/\.$/, '');
      if (host.startsWith('www.')) host = host.slice(4);
      if (!host || (!host.includes('.') && host !== 'localhost')) return null;
      return host;
    } catch {
      return null;
    }
  }

  function addSite() {
    const host = hostOf(siteText.value);
    if (!host) {
      error.value = 'Enter a site like youtube.com';
      return;
    }
    siteText.value = '';
    if (current().sites.includes(host)) return;
    save({ ...current(), sites: [...current().sites, host] });
  }

  function removeSite(site: string) {
    save({ ...current(), sites: current().sites.filter((kept) => kept !== site) });
  }
</script>

<template>
  <section id="section-focus">
    <h2>
      Focus
      <InfoTip
        text="A countdown. The apps below are minimized (hidden on a Mac) while it runs, and with the browser extension the sites are blocked. One session at a time. A break of 0 starts none."
      />
    </h2>
    <p v-if="error" class="notice">{{ error }}</p>
    <label class="field">
      <span>Default length</span>
      <input
        class="focus-minutes"
        type="number"
        min="1"
        :max="WEEK_MINUTES"
        step="1"
        :value="lengthMinutes"
        :disabled="busy"
        aria-label="Default length in minutes"
        @change="onLength"
      />
      <span class="hint-text">minutes, for "focus deep work"</span>
    </label>
    <label class="field">
      <span>Break</span>
      <input
        class="focus-minutes"
        type="number"
        min="0"
        :max="WEEK_MINUTES"
        step="1"
        :value="breakMinutes"
        :disabled="busy"
        aria-label="Break length in minutes"
        @change="onBreak"
      />
      <span class="hint-text">minutes after a session; 0 starts none</span>
    </label>

    <h3>Apps to hide</h3>
    <p class="lede">Minimized on Windows, hidden on a Mac. Nothing is quit.</p>
    <ol v-if="apps.length" class="launch-items">
      <li v-for="app in apps" :key="app.id" class="launch-item">
        <RowIcon :icon="`app:${app.id}`" :label="app.name" :size="20" />
        <span class="launch-item-label">{{ app.name }}</span>
        <button type="button" :disabled="busy" @click="removeApp(app.id)">Remove</button>
      </li>
    </ol>
    <AppPicker :exclude="apps.map((app) => app.id)" placeholder="Add an app" @pick="addApp" />

    <h3>Sites to block</h3>
    <p class="lede">
      A site and its subdomains, while a session runs. Needs the browser extension; without it,
      focus still runs.
    </p>
    <ul v-if="sites.length" class="folders">
      <li v-for="site in sites" :key="site">
        <span class="path">{{ site }}</span>
        <button type="button" :disabled="busy" @click="removeSite(site)">Remove</button>
      </li>
    </ul>
    <form class="field inline" @submit.prevent="addSite">
      <input v-model="siteText" type="text" placeholder="youtube.com" spellcheck="false" />
      <button type="submit" :disabled="busy || !siteText.trim()">Block site</button>
    </form>
  </section>
</template>
