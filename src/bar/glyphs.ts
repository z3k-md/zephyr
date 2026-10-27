// Stroke icons for rows, tokens and the footer, on a 24-unit grid. Each entry is the SVG's
// inner markup; RowIcon draws it with the current text color.

export const GLYPHS: Record<string, string> = {
  search: '<circle cx="11" cy="11" r="6.5"/><path d="M16 16.5 20 20.5"/>',
  clipboard: '<rect x="6" y="4" width="12" height="16" rx="2"/><path d="M9 4h6v3H9z"/>',
  note: '<path d="M6 4h9l3 3v13H6z"/><path d="M9 11h6M9 15h6"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  info: '<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5.5M12 7.75h.01"/>',
  code: '<path d="m8.5 7.5-5 4.5 5 4.5M15.5 7.5l5 4.5-5 4.5M13.5 5l-3 14"/>',
  terminal:
    '<rect x="3.5" y="5" width="17" height="14" rx="2"/><path d="m7.5 10 3 2.5-3 2.5M12.5 15H16"/>',
  // A standard cog: teeth around a hub, so it never reads as a sun or a sparkle.
  gear: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
  file: '<path d="M7 3.5h7l4 4V20.5H7z"/><path d="M14 3.5V8h4"/>',
  folder:
    '<path d="M3.5 7.5a2 2 0 0 1 2-2H10l2 2h6.5a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/>',
  calculator:
    '<rect x="5.5" y="3.5" width="13" height="17" rx="2"/><path d="M8.5 7.5h7M9 12h.01M12 12h.01M15 12h.01M9 16h.01M12 16h.01M15 16h.01"/>',
  conversion: '<path d="M5 8h13l-3-3M19 16H6l3 3"/>',
  clock: '<circle cx="12" cy="12" r="8"/><path d="M12 7.5V12l3 2"/>',
  link: '<path d="M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1"/><path d="M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1"/>',
  sparkle:
    '<path d="M12 3.5 13.8 9a2 2 0 0 0 1.2 1.2l5.5 1.8-5.5 1.8a2 2 0 0 0-1.2 1.2L12 20.5 10.2 15a2 2 0 0 0-1.2-1.2L3.5 12 9 10.2A2 2 0 0 0 10.2 9z"/>',
  prompt: '<path d="m5 7.5 4.5 4.5L5 16.5M12 17h7"/>',
  sync: '<path d="M4.5 10a7.5 7.5 0 0 1 13.4-3.6L20 9M20 4.5V9h-4.5M19.5 14a7.5 7.5 0 0 1-13.4 3.6L4 15M4 19.5V15h4.5"/>',
  back: '<path d="M10 7 5 12l5 5M5 12h14"/>',
  keyboard:
    '<rect x="3" y="6.5" width="18" height="11" rx="2"/><path d="M7 10h.01M10 10h.01M13 10h.01M16 10h.01M7.5 14h9"/>',
  grid: '<rect x="4.5" y="4.5" width="6" height="6" rx="1.2"/><rect x="13.5" y="4.5" width="6" height="6" rx="1.2"/><rect x="4.5" y="13.5" width="6" height="6" rx="1.2"/><rect x="13.5" y="13.5" width="6" height="6" rx="1.2"/>',
  logo: '<path d="M4 8h11a3 3 0 1 0-3-3M4 12h15a3 3 0 1 1-3 3M4 16h7"/>',
  shield:
    '<path d="M12 3.5 5 6.5v5.2c0 4.3 3 7.6 7 8.8 4-1.2 7-4.5 7-8.8V6.5z"/><path d="m9 12 2.2 2.2L15.5 10"/>',
  bookmark: '<path d="M7 3.5h10v17l-5-3.6-5 3.6z"/>',
  network:
    '<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.4 2.3 3.6 5.1 3.6 8.5s-1.2 6.2-3.6 8.5c-2.4-2.3-3.6-5.1-3.6-8.5s1.2-6.2 3.6-8.5z"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  palette:
    '<path d="M12 3.5a8.5 8.5 0 1 0 0 17c1.2 0 1.8-.9 1.8-1.8 0-.5-.2-.9-.5-1.3-.3-.4-.5-.8-.5-1.3 0-1 .8-1.8 1.8-1.8h2.1a3.8 3.8 0 0 0 3.8-3.8C20.5 6.9 16.7 3.5 12 3.5z"/><path d="M7.5 11.5h.01M10 7.5h.01M14.5 7.5h.01"/>',
  // System stats in the footer.
  cpu: '<rect x="6.5" y="6.5" width="11" height="11" rx="1.5"/><rect x="9.5" y="9.5" width="5" height="5" rx=".5"/><path d="M9.5 3.5v3M14.5 3.5v3M9.5 17.5v3M14.5 17.5v3M3.5 9.5h3M3.5 14.5h3M17.5 9.5h3M17.5 14.5h3"/>',
  memory:
    '<rect x="3.5" y="7.5" width="17" height="9" rx="1.5"/><path d="M7 10.5v3M10.3 10.5v3M13.7 10.5v3M17 10.5v3M6.5 16.5v2M17.5 16.5v2"/>',
  // A graphics card from the side: bracket, board with its fan, and the slot's pins.
  gpu: '<path d="M3.5 5.5v14"/><rect x="5.5" y="7.5" width="15" height="9" rx="1.5"/><circle cx="13" cy="12" r="2.5"/><path d="M8.5 16.5v2.5M11.5 16.5v2.5"/>',
  drive:
    '<rect x="3.5" y="13" width="17" height="6.5" rx="1.5"/><path d="M5.5 13 8 5.5h8l2.5 7.5"/><path d="M16.5 16.25h.01"/>',
  updown: '<path d="M8 19V5M4.5 8.5 8 5l3.5 3.5M16 5v14M12.5 15.5 16 19l3.5-3.5"/>',
  // Network links for IP rows: the internet (public), Wi-Fi, a wired port, other LAN.
  globe:
    '<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M5 7.5h14M5 16.5h14"/><path d="M12 3.5c-2.3 2.4-3.4 5.2-3.4 8.5s1.1 6.1 3.4 8.5c2.3-2.4 3.4-5.2 3.4-8.5S14.3 5.9 12 3.5z"/>',
  wifi: '<path d="M3 9.5a13 13 0 0 1 18 0M6 13a8 8 0 0 1 12 0M9 16.4a3.5 3.5 0 0 1 6 0"/><path d="M12 19.5h.01"/>',
  ethernet:
    '<rect x="4" y="5" width="16" height="12" rx="2"/><path d="M8 17v2.5h8V17M8 9v2.5M10.7 9v2.5M13.3 9v2.5M16 9v2.5"/>',
  lan: '<rect x="9" y="3.5" width="6" height="5" rx="1"/><rect x="3.5" y="15.5" width="6" height="5" rx="1"/><rect x="14.5" y="15.5" width="6" height="5" rx="1"/><path d="M12 8.5V12M6.5 15.5V12h11v3.5"/>',
  gamepad:
    '<path d="M7.5 8.5h9a4 4 0 0 1 3.9 4.9l-.8 3.4a2 2 0 0 1-3.5.8l-1.6-2.1h-5l-1.6 2.1a2 2 0 0 1-3.5-.8l-.8-3.4a4 4 0 0 1 3.9-4.9z"/><path d="M8.5 11v3M7 12.5h3M15.5 12h.01M17 13.5h.01"/>',
  // Weather, by WMO code (weather.ts); `moon` stands in for a clear night.
  weatherSun:
    '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6 6 6M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4"/>',
  weatherPartly:
    '<path d="M9 3.5V5M4.4 5.4l1 1M2.8 10h1.5"/><path d="M6.6 11.6A4 4 0 0 1 13.4 7.8"/><path d="M9 20h8.5a3.5 3.5 0 0 0 .4-7 5 5 0 0 0-9.6 1.2A3 3 0 0 0 9 20z"/>',
  weatherPartlyNight:
    '<path d="M11.8 4.3a4.6 4.6 0 0 0-5.4 6.8"/><path d="M9 20h8.5a3.5 3.5 0 0 0 .4-7 5 5 0 0 0-9.6 1.2A3 3 0 0 0 9 20z"/>',
  weatherCloud: '<path d="M7 18h10a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.2 9.1 4.5 4.5 0 0 0 7 18z"/>',
  weatherFog: '<path d="M4 8h16M6 12h12M4 16h16M8 20h8"/>',
  weatherDrizzle:
    '<path d="M7 15h10a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.2 6.1 4.5 4.5 0 0 0 7 15z"/><path d="M8 19h.01M12 19h.01M16 19h.01"/>',
  weatherRain:
    '<path d="M7 15h10a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.2 6.1 4.5 4.5 0 0 0 7 15z"/><path d="M8 18l-1 2.5M12 18l-1 2.5M16 18l-1 2.5"/>',
  weatherSnow:
    '<path d="M7 15h10a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.2 6.1 4.5 4.5 0 0 0 7 15z"/><path d="M8 18.5h.01M12 18.5h.01M16 18.5h.01M10 21h.01M14 21h.01"/>',
  weatherStorm:
    '<path d="M7 15h10a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.2 6.1 4.5 4.5 0 0 0 7 15z"/><path d="m12.5 15.5-2 3.5h3l-2 3.5"/>',
  // Google's G as a filled silhouette in the row color, for recent Google searches.
  google:
    '<g transform="translate(4 4) scale(.3333)" fill="currentColor" stroke="none"><path d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z"/><path d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z"/><path d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z"/><path d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z"/></g>',
  music:
    '<path d="M9 17.5V6l10-2v11.5"/><circle cx="6.5" cy="17.5" r="2.5"/><circle cx="16.5" cy="15.5" r="2.5"/>',
  power: '<path d="M12 3.5v8"/><path d="M7.2 6.2a7.5 7.5 0 1 0 9.6 0"/>',
  chip: '<rect x="6.5" y="6.5" width="11" height="11" rx="1.5"/><path d="M9.5 3.5v3M14.5 3.5v3M9.5 17.5v3M14.5 17.5v3M3.5 9.5h3M3.5 14.5h3M17.5 9.5h3M17.5 14.5h3"/>',
  moon: '<path d="M19.5 14.5A8 8 0 0 1 9.5 4.5a8 8 0 1 0 10 10z"/>',
  lock: '<rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5"/>',
  restart: '<path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3L4.5 9"/><path d="M4.5 4.5V9H9"/>',
  signout:
    '<path d="M14 4.5H6.5a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2H14"/><path d="M10 12h10M16.5 8.5 20 12l-3.5 3.5"/>',
};
