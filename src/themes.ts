// The palettes in Settings and first-run setup; ids match THEMES in state.rs (and stay, so
// a saved choice survives a rename). `lightTint` is the swatch in light mode.
export const THEMES = [
  { id: 'sidebar', name: 'Glass', tint: '#2b2b30', lightTint: '#f2f2f4', accent: '#3478f6' },
  { id: 'violet', name: 'Dusk', tint: '#16111f', lightTint: '#f1edf9', accent: '#a78bfa' },
  { id: 'graphite', name: 'Graphite', tint: '#18181b', lightTint: '#f1f1f2', accent: '#d4d4d8' },
  { id: 'ocean', name: 'Ocean', tint: '#0c1424', lightTint: '#eaf1fb', accent: '#60a5fa' },
  { id: 'forest', name: 'Forest', tint: '#0a1b19', lightTint: '#e8f5f2', accent: '#2dd4bf' },
  { id: 'ember', name: 'Ember', tint: '#20130c', lightTint: '#fbefe7', accent: '#fb923c' },
];

// How the windows are drawn, on either platform; ids match STYLES in state.rs.
export const STYLES = [
  { id: 'glass', name: 'Zephyr' },
  { id: 'windows', name: 'Windows' },
  { id: 'mac', name: 'Mac' },
];

// Ids match MODES in state.rs.
export const MODES = [
  { id: 'dark', name: 'Dark' },
  { id: 'light', name: 'Light' },
  { id: 'system', name: 'Match system' },
];

export const LIGHT_SCHEME = '(prefers-color-scheme: light)';

const systemLight = () =>
  typeof window !== 'undefined' && window.matchMedia?.(LIGHT_SCHEME).matches === true;

/** What a saved mode looks like: `system` follows the OS (asked now unless given). */
export function resolveMode(
  mode: string | undefined,
  osLight: boolean = systemLight()
): 'dark' | 'light' {
  if (mode === 'light') return 'light';
  if (mode === 'system') return osLight ? 'light' : 'dark';
  return 'dark';
}
