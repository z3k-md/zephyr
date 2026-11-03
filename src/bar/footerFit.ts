// How the footer's fixed pieces share the bar's 680px. The width is constant, so
// what gives way depends on which pieces are showing, in this order: the focus
// progress bar, the meter bars, the meter numbers, then a shorter now-playing
// title. Text that already ellipsizes (the song, the focus name, the meeting,
// the condition) is counted at the width it can shrink to without cutting a glyph.

/** Width the pieces can use. Playing trims the footer's left padding; 4px is spare
 *  for rounding. The bar is 680px with a 1px border on each side. */
export function footerRoom(playing: boolean): number {
  return 678 - (playing ? 8 : 16) - 16 - 4;
}

export interface FooterPresence {
  playing: boolean;
  focus: boolean;
  /** CPU and memory. */
  meters: boolean;
  gpu: boolean;
  /** The low-disk warning. It is never dropped. */
  disk: boolean;
  net: boolean;
  meeting: boolean;
  weather: boolean;
}

/** 0 keeps every piece. Higher values have given way through that step. */
export type FooterFit = 0 | 1 | 2 | 3 | 4;

const PLAYING = 300;
const PLAYING_SHORT = 168;
const FOCUS_REST = 84;
const FOCUS_BAR = 42;
const METER_FULL = 85;
const METER_NO_BAR = 51;
const METER_ICON = 13;
const METER_GAP = 14;
const DISK = 72;
const NET = 90;
const MEETING = 64;
const WEATHER = 60;
const DIVIDER = 1;

function gap(focus: boolean, level: FooterFit): number {
  if (level >= 4) return 4;
  if (level >= 3) return 8;
  return focus ? 12 : 18;
}

function metersWidth(present: FooterPresence, level: FooterFit): number {
  const stats = 2 + (present.gpu ? 1 : 0);
  const one = level >= 3 ? METER_ICON : level >= 2 ? METER_NO_BAR : METER_FULL;
  let total = one * stats + METER_GAP * (stats - 1);
  if (present.disk) total += METER_GAP + DISK;
  return total;
}

/** The width the footer needs at this step. Above `footerRoom`, the next step gives way. */
export function footerWidth(present: FooterPresence, level: FooterFit): number {
  const pieces: number[] = [];
  if (present.playing) pieces.push(level >= 4 ? PLAYING_SHORT : PLAYING);
  if (present.playing && (present.focus || present.meters)) pieces.push(DIVIDER);
  if (present.focus) pieces.push(FOCUS_REST + (level >= 1 ? 0 : FOCUS_BAR));
  if (present.focus && present.meters) pieces.push(DIVIDER);
  if (present.meters) pieces.push(metersWidth(present, level));
  if (present.net) pieces.push(NET);
  if (present.meeting) pieces.push(MEETING);
  if (present.weather) pieces.push(WEATHER);
  const gaps = Math.max(0, pieces.length - 1) * gap(present.focus, level);
  return pieces.reduce((sum, piece) => sum + piece, 0) + gaps;
}

/** The first step at which the pieces that are showing fit the bar. */
export function footerFit(present: FooterPresence): FooterFit {
  for (const level of [0, 1, 2, 3, 4] as const) {
    if (footerWidth(present, level) <= footerRoom(present.playing)) return level;
  }
  return 4;
}
