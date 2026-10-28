import { createApp } from 'vue';
import { listen } from '@tauri-apps/api/event';
import App from './App.vue';
import { captureSnapshot, captureToggle, getSnapshot } from './api';
import { isSnapshot, type Snapshot } from './types';
import { pickedUnit } from './bar/weather';
import './styles.css';

// Lets the styles follow the native window chrome, which differs between platforms.
if (navigator.userAgent.includes('Windows')) document.documentElement.dataset.platform = 'windows';
else if (navigator.userAgent.includes('Mac')) document.documentElement.dataset.platform = 'macos';

// Right-click outside text would open the browser's own menu (Back, Refresh, Print), which
// has no place in an app; text keeps it for copy, paste and spelling. Dev builds keep it
// everywhere, for Inspect.
if (!import.meta.env.DEV) {
  window.addEventListener('contextmenu', (event) => {
    const target = event.target as HTMLElement | null;
    if (!target?.closest('input, textarea, [contenteditable]:not([contenteditable="false"])')) {
      event.preventDefault();
    }
  });
}

// Bug captures (Settings › Dev): Ctrl+Shift+S snapshots and Ctrl+R records the Zephyr window
// in front (Ctrl+S opens Settings from the bar). They're this page's keys, not global ones, so
// every other app keeps its own. Registered before any view's key handling, so no view swallows them first.
let bugCapture = true;
window.addEventListener(
  'keydown',
  (event) => {
    if (!bugCapture || event.repeat || !event.ctrlKey) return;
    if (event.metaKey || event.altKey) return;
    const snap = event.code === 'KeyS' && event.shiftKey;
    const record = event.code === 'KeyR' && !event.shiftKey;
    if (!snap && !record) return;
    // A shortcut being recorded takes them as the new shortcut.
    if (document.querySelector('.recorder.listening')) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    void (snap ? captureSnapshot() : captureToggle()).catch(() => undefined);
  },
  true
);

// Windows: the webview hands Alt+Space to the window menu, so the page never sees it. While
// a shortcut is being recorded, the window passes it on (window.rs) and it arrives here as
// the keypress it was, for whichever recorder is listening.
void listen<{ ctrl: boolean; shift: boolean; meta: boolean }>('alt-space', (event) => {
  if (!document.querySelector('.recorder.listening')) return;
  const { ctrl, shift, meta } = event.payload;
  window.dispatchEvent(
    new KeyboardEvent('keydown', {
      key: ' ',
      code: 'Space',
      altKey: true,
      ctrlKey: ctrl,
      shiftKey: shift,
      metaKey: meta,
      bubbles: true,
      cancelable: true,
    })
  );
});

// The palette from Settings > Appearance and the weather's unit, in every window, switching
// live when they change.
function apply(snapshot: Snapshot) {
  document.documentElement.dataset.theme = snapshot.theme || 'sidebar';
  bugCapture = snapshot.bugCapture;
  pickedUnit.value = snapshot.weatherUnit;
}
void getSnapshot()
  .then(apply)
  .catch(() => undefined);
void listen<unknown>('state-changed', (event) => {
  if (isSnapshot(event.payload)) apply(event.payload);
});

createApp(App).mount('#app');
