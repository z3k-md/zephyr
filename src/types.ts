interface HistoryEntry {
  query: string;
  /** 'google' for a search, 'url' for a link. */
  destinationId: string;
  uses: number;
  lastUsed: number;
}

export interface Snapshot {
  version: number;
  summonShortcut: string;
  launchAtStartup: boolean;
  /** Windows: start from a logon task right at sign-in (opt-in; Defender may flag it). */
  instantStartup: boolean;
  history: HistoryEntry[];
  launches: LaunchEntry[];
  appOverrides: string[];
  fileRoots: string[] | null;
  fileExcludes: string[];
  fileOpens: LaunchEntry[];
  /** Views opened, by view id, for the empty bar's Views. */
  viewOpens: LaunchEntry[];
  clipboard: ClipboardSettings;
  notesShortcut: string;
  typingBests: TypingBest[];
  resumeSeconds: number;
  activeMode: string;
  returnToMode: boolean;
  shell: ShellSettings;
  /** The color palette, e.g. violet or ocean. */
  theme: string;
  /** A power action set for later from the bar. */
  power: PowerSchedule | null;
  showStats: boolean;
  onboardingDone: boolean;
  onboardingEveryLaunch: boolean;
  launchGroups: LaunchGroup[];
  bugCapture: boolean;
  protectedFolders: boolean;
  /** Hotkeys and aliases the user gave commands, apps and bookmarks. */
  bindings: Binding[];
  /** Where the weather is for; null uses where the IP address says. */
  weatherPlace: Place | null;
  /** Celsius or Fahrenheit; null follows the region's own way. */
  weatherUnit: WeatherUnit | null;
  /** With no city picked, whether the weather asks ipapi.co where the IP address is. */
  weatherByIp: boolean;
  /** Whether the connection monitor pings the router and 1.1.1.1 once a second. */
  watchConnection: boolean;
  lastSpeed: SpeedResult | null;
  /** Windows: the CPU temperature helper (PawnIO) is turned on. */
  cpuTemp: boolean;
  /** Windows: Zephyr takes the volume keys and shows its own overlay. */
  volumeKeys: boolean;
}

export type WeatherUnit = 'c' | 'f';

/** A place weather is for. */
export interface Place {
  name: string;
  /** State or province, when there is one. */
  region: string;
  country: string;
  latitude: number;
  longitude: number;
}

/** Weather from Open-Meteo, all metric (°C, km/h). */
export interface Forecast {
  place: Place;
  fetchedAt: number;
  /** The place's offset from UTC, so its days and hours read in its own time. */
  utcOffsetSeconds: number;
  current: { temperature: number; feelsLike: number; code: number; wind: number; isDay: boolean };
  /** The next 48 hours; `time` in seconds since the epoch. */
  hourly: {
    time: number;
    temperature: number;
    code: number;
    precipitation: number;
    isDay: boolean;
  }[];
  /** Today and the next six days; `time` is midnight at the place. */
  daily: {
    time: number;
    code: number;
    high: number;
    low: number;
    precipitation: number;
    uv: number;
  }[];
}

/** The song or video playing on this computer. */
export interface NowPlaying {
  title: string;
  artist: string;
  album: string;
  /** The app playing it (an AppUserModelID on Windows, such as Spotify.exe or Chrome). */
  sourceApp: string;
  playing: boolean;
  positionMs: number | null;
  durationMs: number | null;
  /** The cover as a data: URL. */
  artwork: string | null;
  updatedAt: number;
  /** When the app was last seen playing, in ms since the epoch. */
  lastPlayedAt: number | null;
}

export type MediaControl = 'toggle' | 'next' | 'previous';

export type ShellProgram = 'powershell' | 'gitbash' | 'wsl';

export interface ShellSettings {
  program: ShellProgram;
  /** Commands run from the bar, newest first. */
  history: string[];
}

export interface ShellInfo {
  /** Shells found on this PC; empty on macOS, which always uses the login shell. */
  available: ShellProgram[];
  using: string | null;
  /** The shell in use, which may stand in for a chosen one that isn't installed. */
  program: ShellProgram | null;
  /** Where Ctrl+Enter sends a command, e.g. "Windows Terminal". */
  terminal: string;
}

export type ShellEvent =
  | { kind: 'started'; id: number; shell: string }
  | { kind: 'output'; text: string }
  | { kind: 'exit'; code: number | null; stopped: boolean; millis: number }
  | { kind: 'error'; message: string };

export interface TypingBest {
  mode: string;
  wpm: number;
  raw: number;
  accuracy: number;
  consistency: number;
  at: number;
}

export interface ClipboardSettings {
  enabled: boolean;
  shortcut: string;
  retentionDays: number;
  ignoredApps: string[];
  recognizeText: boolean;
}

export type ClipKind = 'text' | 'link' | 'color' | 'image' | 'files';

interface ClipImage {
  width: number;
  height: number;
  bytes: number;
}

export interface ClipSummary {
  id: number;
  kind: ClipKind;
  title: string;
  source: string | null;
  lastCopied: number;
  copies: number;
  pinned: boolean;
  color?: string;
  image?: ClipImage | null;
}

export interface ClipList {
  items: ClipSummary[];
  problem: string | null;
  enabled: boolean;
}

export interface ClipDetail {
  id: number;
  kind: ClipKind;
  text: string;
  files: string[];
  imageUrl: string | null;
  image: ClipImage | null;
  ocr: string | null;
  source: string | null;
  firstCopied: number;
  lastCopied: number;
  copies: number;
  pinned: boolean;
  color: string | null;
}

export interface FileIndexStatus {
  roots: string[];
  entries: number;
  scanning: boolean;
}

export interface LaunchEntry {
  appId: string;
  uses: number;
  lastUsed: number;
}

/** A result as Rust sends it (suggest.rs). */
export interface Suggestion {
  label: string;
  query: string;
  /** What the row opens: for a search row 'web' (a Google search of the query) or 'url', for
   *  a command row its view or command; empty otherwise. */
  target: string;
  kind:
    | 'history'
    | 'remote'
    | 'app'
    | 'setting'
    | 'file'
    | 'answer'
    | 'note'
    | 'noteNew'
    | 'view'
    | 'query'
    | 'power'
    | 'bookmark'
    | 'powerPending'
    | 'netCard'
    | 'portCard'
    | 'port'
    | 'window';
  hint: string;
  appId?: string;
  settingId?: string;
  path?: string;
  noteId?: string;
  /** For a port row: the socket and the process that has it open. */
  portUse?: PortUse;
  /** The section it's listed under (apps, web, files…); jump, power and shell come from the
   *  bar itself. */
  section?: string;
  subtitle?: string;
  /** app:<id>, site:<url>, glyph:<name>, or zephyr for the app's own icon. */
  icon?: string;
  /** A browser's private window (Incognito, InPrivate); Shift+Enter opens one. */
  privateWindow?: string;
  /** An open app's windows, front to back; Enter switches to the first. */
  windows?: WindowRef[];
  /** The app is running though its windows aren't listed (macOS), so it can be quit. */
  running?: boolean;
  /** The window a window row switches to. */
  window?: number;
}

/** A row as the bar lists it: a result, or one the bar adds itself (views, commands, power,
 *  the search row), with what only the bar draws. */
export interface SuggestionRow extends Suggestion {
  /** Small icons for what a row holds (a launch group's apps). */
  icons?: string[];
  /** Keycaps shown instead of the hint. */
  keys?: string[];
  /** Key hints ("enter" Search, "shift+enter" AI Mode) shown instead of the hint, which
   *  keeps the same words as plain text. */
  keyHints?: { keys: string; label: string }[];
  /** For power rows: what Enter runs. */
  power?: PowerPreview;
}

export interface NoteSummary {
  id: string;
  title: string;
  snippet: string;
  updated: number;
}

export interface Note {
  id: string;
  body: string;
  /** 'markdown' for a note saved by an earlier build. */
  format: 'html' | 'markdown';
  updated: number;
}

export interface SuggestResponse {
  mode: 'search' | 'recent';
  items: Suggestion[];
  notice: string | null;
  preselect: number | null;
  /** Where free text goes: 'web' for a Google search, or 'url'. */
  target: string | null;
  text: string;
  /** Whether the network pass would add anything; when not, the bar skips it. */
  remote?: boolean;
}

export type DispatchOutcome =
  | { kind: 'opened' }
  | { kind: 'launched'; appId: string }
  | { kind: 'settingOpened'; settingId: string }
  | { kind: 'fileOpened'; path: string }
  | { kind: 'empty' };

export function isSnapshot(value: unknown): value is Snapshot {
  if (typeof value !== 'object' || value === null) return false;
  return Array.isArray((value as Snapshot).history);
}

export type SyncPhase =
  | { kind: 'signedOut' }
  | { kind: 'browser' }
  | { kind: 'showRecovery'; recovery: string }
  | { kind: 'awaitingApproval'; words: string }
  | { kind: 'ready' };

interface SyncDevice {
  id: string;
  platform: string;
  displayName: string;
  lastSeenAt: string | null;
  revokedAt: string | null;
  createdAt: string | null;
}

export type PermissionId = 'accessibility' | 'automation' | 'files';
export interface Permission {
  id: PermissionId;
  status: 'granted' | 'denied' | 'notAsked' | 'unknown';
}

export interface SyncStatus {
  phase: SyncPhase;
  email: string;
  deviceId: string;
  deviceName: string;
  words: string;
  keyGen: number | null;
  devices: SyncDevice[];
}

export interface SyncPending {
  id: string;
  name: string;
  platform: string;
  words: string;
}

export type PowerAction = 'shutdown' | 'restart' | 'sleep' | 'lock' | 'sign_out' | 'firmware';

export interface PowerSchedule {
  action: PowerAction;
  /** Unix seconds. */
  at: number;
}

export interface PowerPreview {
  action: PowerAction;
  label: string;
  at: number | null;
  cancel: boolean;
  confirm: boolean;
  /** The text names the action outright, so Enter may take the row. */
  exact: boolean;
}

/** The update card: downloading, counting down to install, installing, or failed. */
export type UpdateStatus =
  | { phase: 'downloading'; version: string; downloaded: number; total: number | null }
  | { phase: 'ready'; version: string; installAt: number }
  | { phase: 'installing'; version: string }
  | { phase: 'failed'; version: string; message: string };

/** An open window, listed under its app's row. */
export interface WindowRef {
  handle: number;
  title: string;
}

/**
 * A hotkey and/or alias for one thing the bar can do. target: view:clip, scope:files,
 * app:<id>, bookmark:<url>, group:<id> or setting:<id>.
 */
export interface Binding {
  target: string;
  label: string;
  hotkey: string;
  alias: string;
  /** Fires as soon as the alias is typed, with no space. */
  direct: boolean;
}

/** What the bar's preview pane shows for a file result. */
export type FilePreview =
  | { kind: 'image'; url: string; size: number; modified: number }
  | { kind: 'text'; text: string; truncated: boolean; size: number; modified: number }
  | { kind: 'info'; size: number; modified: number; folder: boolean };

/** The GPU: its busiest engine, and the main GPU's own memory. */
interface GpuStats {
  /** Empty where it isn't known (macOS). */
  name: string;
  /** 0–100. */
  usage: number;
  memoryUsed: number;
  /** 0 where the GPU shares the computer's memory (Apple silicon). */
  memoryTotal: number;
}

/** Live system stats for the bar's footer. */
export interface SystemStats {
  cpu: number;
  memoryUsed: number;
  memoryTotal: number;
  diskFree: number;
  diskTotal: number;
  netDown: number;
  netUp: number;
  /** Absent without a GPU that reports its load. */
  gpu?: GpuStats;
  /** The CPU and the GPU, in °C; absent where they can't be read. */
  cpuTemp?: number | null;
  gpuTemp?: number | null;
  /** Trouble on the line, for the footer's chip. */
  network?: NetworkAlert;
  /** Dev builds: the app's own use, webview helpers included. */
  app?: { cpu: number; memory: number; processes: number };
}

/** One app, or one program, in System Resources: every process it's made of, summed. */
export interface ResourceRow {
  /** Stable across ticks, so the selection follows the app, not its position. */
  key: string;
  name: string;
  /** A lone command-line program: what it runs and where ("bun dev · ~/code/zephyr"). */
  subtitle: string;
  icon: string;
  pids: number[];
  /** Percent of one core (macOS) or of the machine (Windows); null when it can't be read. */
  cpu: number | null;
  memory: number | null;
  /** Bytes read and written per second. */
  disk: number | null;
  /** Windows: its busiest GPU engine, 0–100; null elsewhere. */
  gpu?: number | null;
  user: string;
  path: string;
  command: string;
  /** The system needs it, so it can't be force quit. */
  protected: boolean;
  /** Finder, the Dock, Explorer: relaunched rather than quit. */
  relaunch: boolean;
  /** Its use can't be read without administrator rights. */
  locked: boolean;
  /** An app with a window the user works in; these list first, as Apps. */
  windowed: boolean;
}

export interface ResourcePoint {
  /** Seconds ago. */
  age: number;
  cpu: number;
  /** Share of memory in use, 0 to 1. */
  memory: number;
  /** The GPU's load, 0–100; 0 without one. */
  gpu: number;
  down: number;
  up: number;
}

export interface ResourceSnapshot {
  totals: SystemStats;
  cores: number;
  /** The last 15 minutes, a point a second, oldest first. */
  history: ResourcePoint[];
  rows: ResourceRow[];
  processes: number;
  /** Seconds since the computer started. */
  uptime: number;
  /** Absent without a battery. */
  battery?: Battery;
}

export type ResourceSort = 'cpu' | 'memory' | 'disk' | 'gpu' | 'name';

/** Things opened together from one row or hotkey ("Coding", "Gaming"). */
export interface LaunchGroup {
  id: string;
  name: string;
  /** A glyph name; empty uses the first app's icon. */
  icon: string;
  items: GroupItem[];
}

export interface GroupItem {
  kind: 'app' | 'url' | 'folder';
  value: string;
  label: string;
}

/** Trouble on the line: much slower than usual, probes lost in a row, or no route at all. */
type NetworkAlert = { kind: 'slow'; ms: number } | { kind: 'drop' } | { kind: 'offline' };

export type NetworkGrade = 'good' | 'fair' | 'poor';

/** The connection over the last 15 minutes, for the network card. */
export interface NetworkSummary {
  watching: boolean;
  /** Whether anything has been sent to the internet yet. */
  measured: boolean;
  grade: NetworkGrade;
  median: number | null;
  jitter: number | null;
  /** 0–1. */
  loss: number;
  verdict: string;
  offline: boolean;
  /** The internet's median for each of 180 five-second stretches, oldest first. */
  spark: (number | null)[];
  /** Which of those stretches lost probes. */
  sparkLost: boolean[];
  gateway: string | null;
  dns: string | null;
  /** "Wi-Fi" or "network". */
  link: string;
  alert: NetworkAlert | null;
}

export interface HopStats {
  median: number | null;
  p95: number | null;
  jitter: number | null;
  loss: number;
  sent: number;
  /** The latest probe's time; null when it got no answer or none was sent. */
  last: number | null;
  lastLost: boolean;
}

/** Five seconds of the chart. `flags`: 1 gap, 2 network change, 4 TCP timing, 8 speed test,
 *  16 offline. */
export interface NetworkBucket {
  median: number | null;
  p95: number | null;
  lost: number;
  sent: number;
  down: number;
  up: number;
  flags: number;
}

export interface NetworkHistory {
  summary: NetworkSummary;
  buckets: NetworkBucket[];
  router: HopStats;
  internet: HopStats;
  dnsHop: HopStats;
  drops: { ago: number; seconds: number; pastRouter: boolean }[];
  usedDown: number;
  usedUp: number;
  peakDown: number;
  peakUp: number;
}

export interface NetworkAddress {
  value: string;
  detail: string;
  icon: string;
}

export interface SpeedResult {
  /** When it ran, ms since 1970. */
  at: number;
  downMbps: number;
  upMbps: number;
  idleMs: number | null;
  downMs: number | null;
  upMs: number | null;
  /** A to D, or – without pings. */
  bloat: string;
}

export interface SpeedProgress {
  phase: 'ping' | 'download' | 'upload';
  mbps: number | null;
  ping: number | null;
  /** 0–1 through the phase. */
  done: number;
}

/** An open socket and the process behind it ("port 3000"). */
export interface PortUse {
  port: number;
  /** TCP or UDP. */
  protocol: string;
  /** "127.0.0.1:3000", "[::1]:3000", "*:3000". */
  local: string;
  /** The other end of a connection; empty for a listener. */
  remote: string;
  /** LISTEN, ESTABLISHED…; empty for UDP. */
  state: string;
  pid: number;
  /** Empty when the process can't be read. */
  name: string;
  command: string;
  path: string;
  icon: string;
}

export interface Battery {
  /** 0–100. */
  percent: number;
  state: 'charging' | 'discharging' | 'full' | 'unknown';
  /** Until empty or full, where the system estimates it. */
  secondsLeft: number | null;
}

/** Show info for one Resources row. */
/** One process in an expanded Resources row. */
export interface ResourceProcess {
  pid: number;
  name: string;
  /** What it does: Chromium's process type ("renderer"), else its first arguments. */
  role: string;
  cpu: number | null;
  memory: number | null;
  disk: number | null;
  gpu: number | null;
}

export interface ResourceDetail {
  key: string;
  pid: number;
  parent: string;
  /** Seconds since 1970. */
  started: number;
  read: number;
  written: number;
  /** What it was started with; empty where it can't be read. */
  command: string;
  /** Oldest first: seconds ago, CPU %, memory bytes. */
  history: [number, number, number][];
}

/** The CPU drill-in: each logical processor's load in 90 ten-second stretches. */
export interface CoreHeat {
  cores: number;
  /** columns[time][core], 0–100; null before Zephyr was reading. */
  columns: (number[] | null)[];
  now: number[];
}

/** The Memory drill-in (Windows). */
export interface MemoryBreakdown {
  total: number;
  inUse: number;
  modified: number;
  standby: number;
  free: number;
  committed: number;
  commitLimit: number;
  pagedPool: number;
  nonpagedPool: number;
  /** macOS: resident and compressed memory, which Windows doesn't report. */
  wired?: number;
  compressed?: number;
}

/** A stock or crypto quote for the ticker card. */
export interface Quote {
  symbol: string;
  name: string;
  currency: string;
  exchange: string;
  crypto: boolean;
  price: number;
  /** What the change is measured from: the close before the chart starts, or for crypto's
   *  day the price 24 hours ago. */
  reference: number;
  range: QuoteRange;
  /** The last trade, in milliseconds since the epoch. */
  tradedAt: number;
  /** [seconds since the epoch, price], oldest first. */
  points: [number, number][];
  fetchedAt: number;
}

/** A ticker chart's span. */
export type QuoteRange = '1d' | '5d' | '1mo' | '1y';

/** An output device, as Windows' sound settings name it. */
export interface SoundDevice {
  id: string;
  /** "Speakers", "Headphones". */
  name: string;
  /** The hardware: "Realtek High Definition Audio". */
  detail: string;
}

/** One app in the volume mixer, all its sessions together. */
export interface AppAudio {
  /** What the setters take: the program's path in lowercase, or `system`. */
  key: string;
  name: string;
  icon: string;
  /** 0–1. */
  volume: number;
  muted: boolean;
  /** 0–1, how loud it is right now. */
  peak: number;
  /** Playing now, rather than open and quiet. */
  active: boolean;
}

/** The default output device and what's on it. */
export interface Sound {
  device: SoundDevice | null;
  devices: SoundDevice[];
  volume: number;
  muted: boolean;
  peak: number;
  apps: AppAudio[];
}

/** A level after a change: master (`key` = `master`) or an app. */
export interface SoundLevel {
  key: string;
  name: string;
  icon: string;
  volume: number;
  muted: boolean;
  /** Playing now (master always is). */
  active: boolean;
}

/** After a volume key: the keys' target and every level, for the overlay and the bar's drawer. */
export interface VolumeShown {
  /** `master` or an app's key. */
  key: string;
  /** Master first, then the apps. */
  levels: SoundLevel[];
}
