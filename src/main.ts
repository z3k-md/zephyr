import { createApp } from 'vue';
import { listen } from '@tauri-apps/api/event';
import App from './App.vue';
import { getSnapshot } from './api';
import { isSnapshot } from './types';
import './styles.css';

// Lets the styles follow the native window chrome, which differs between platforms.
if (navigator.userAgent.includes('Windows')) document.documentElement.dataset.platform = 'windows';

// The palette from Settings > Appearance, in every window, switching live when it changes.
function applyTheme(theme: string | undefined) {
  document.documentElement.dataset.theme = theme || 'sidebar';
}
void getSnapshot()
  .then((snapshot) => applyTheme(snapshot.theme))
  .catch(() => undefined);
void listen<unknown>('state-changed', (event) => {
  if (isSnapshot(event.payload)) applyTheme(event.payload.theme);
});

createApp(App).mount('#app');
