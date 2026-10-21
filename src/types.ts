export type SuggestKind = 'none' | 'google' | 'youtube' | 'wikipedia' | 'pubmed' | 'custom';

export interface Destination {
  id: string;
  name: string;
  triggers: string[];
  urlTemplate: string;
  suggest: SuggestKind;
  suggestUrl?: string;
  suggestPath?: string;
  pinned: boolean;
  builtin: boolean;
  disabled: boolean;
  kind?: 'web' | 'ai';
}

export type AiApi = 'openai' | 'anthropic';

export interface AiSettings {
  provider: string;
  model: string;
  baseUrl: string;
  api: AiApi;
}

export interface AiPreset {
  id: string;
  name: string;
  api: AiApi;
  baseUrl: string;
  needsKey: boolean;
  local: boolean;
  defaultModel: string;
}

export interface LocalServer {
  provider: string;
  name: string;
  models: string[];
}

export type AiEvent =
  | { kind: 'started'; provider: string; model: string }
  | { kind: 'delta'; text: string }
  | { kind: 'done' }
  | { kind: 'error'; message: string };

export interface HistoryEntry {
  query: string;
  destinationId: string;
  uses: number;
  lastUsed: number;
}

export interface Snapshot {
  version: number;
  summonShortcut: string;
  launchAtStartup: boolean;
  defaultDestinationId: string;
  destinations: Destination[];
  history: HistoryEntry[];
  launches: LaunchEntry[];
  appOverrides: string[];
  fileRoots: string[] | null;
  fileExcludes: string[];
  fileOpens: LaunchEntry[];
  ai: AiSettings;
  clipboard: ClipboardSettings;
  notesShortcut: string;
  typingBests: TypingBest[];
  claude: ClaudeSettings;
  resumeSeconds: number;
  activeMode: string;
  returnToMode: boolean;
  routeCmd: string;
  routeAlt: string;
  shell: ShellSettings;
  /** The color palette, e.g. violet or ocean. */
  theme: string;
  /** A power action set for later from the bar. */
  power: PowerSchedule | null;
  /** Browser profiles whose bookmarks stay out of the bar. */
  bookmarksDisabled: string[];
  /** Whether the bar's footer shows the song or video playing. */
  showNowPlaying: boolean;
  showStats: boolean;
  /** The one AI the bar offers (a destination id); empty for the first AI listed. */
  aiChoice: string;
  onboardingDone: boolean;
  onboardingEveryLaunch: boolean;
  launchGroups: LaunchGroup[];
  bugCapture: boolean;
  protectedFolders: boolean;
  /** Whether installed Steam games are listed with the apps. */
  steamGames: boolean;
  /** Hotkeys and aliases the user gave commands, apps, destinations and bookmarks. */
  bindings: Binding[];
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

export type ShellProgram = 'auto' | 'powershell' | 'gitbash' | 'wsl';

export interface ShellSettings {
  program: ShellProgram;
  /** Commands run from the bar, newest first. */
  history: string[];
  /** Windows: 'auto' sends Enter to Windows Terminal when installed, 'console' to a console. */
  terminal: 'auto' | 'console';
}

export interface ShellInfo {
  /** Shells found on this PC; empty on macOS, which always uses the login shell. */
  available: ShellProgram[];
  using: string | null;
  /** Where Enter sends a command, e.g. "Windows Terminal". */
  terminal: string;
  hasWindowsTerminal: boolean;
}

export type ShellEvent =
  | { kind: 'started'; id: number; shell: string }
  | { kind: 'output'; text: string }
  | { kind: 'exit'; code: number | null; stopped: boolean; millis: number }
  | { kind: 'error'; message: string };

export type ClaudeProfile = 'edit' | 'auto' | 'plan';

export interface ClaudeProject {
  id: string;
  folder: string;
  alias: string;
  profile: ClaudeProfile;
  allow: string[];
  model: string;
  effort: string;
  note: string;
}

export interface ClaudeSettings {
  binary: string;
  projects: ClaudeProject[];
  lastProject: string;
  maxTurns: number;
  timeoutMinutes: number;
  approvalMinutes: number;
  notifications: boolean;
}

export type ClaudeStatus =
  'queued' | 'running' | 'waiting' | 'done' | 'failed' | 'cancelled' | 'interrupted';

export interface ClaudeTurn {
  prompt: string;
  started: number | null;
  ended: number | null;
  result: string;
  denials: string[];
  exit: number | null;
}

export interface ClaudeJob {
  id: string;
  session: string;
  projectId: string;
  project: string;
  folder: string;
  title: string;
  status: ClaudeStatus;
  activity: string;
  summary: string;
  created: number;
  updated: number;
  turns: ClaudeTurn[];
}

export interface ClaudeApproval {
  id: string;
  jobId: string;
  project: string;
  tool: string;
  summary: string;
  detail: string;
  created: number;
}

export interface ClaudeCliStatus {
  path: string | null;
  version: string | null;
  loggedIn: boolean | null;
  authMethod: string | null;
}

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

export interface ClipImage {
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

export interface Suggestion {
  label: string;
  query: string;
  destinationId: string;
  kind:
    | 'history'
    | 'remote'
    | 'destination'
    | 'app'
    | 'setting'
    | 'file'
    | 'answer'
    | 'note'
    | 'noteNew'
    | 'view'
    | 'query'
    | 'searchWith'
    | 'power'
    | 'bookmark'
    | 'powerPending'
    | 'window';
  hint: string;
  appId?: string;
  settingId?: string;
  path?: string;
  noteId?: string;
  /** apps, answer, recent, web, files, settings, notes, destinations; jump and searchWith
   *  come from the bar itself. */
  section?: string;
  subtitle?: string;
  /** app:<id>, dest:<id> or glyph:<name>. */
  icon?: string;
  /** A browser's private window (Incognito, InPrivate); Shift+Enter opens one. */
  privateWindow?: string;
  /** Small icons for what a row holds (a launch group's apps). */
  icons?: string[];
  /** Keycaps shown instead of the hint. */
  keys?: string[];
  /** For power rows: what Enter runs. */
  power?: PowerPreview;
  /** An open app's windows, front to back; Enter switches to the first. */
  windows?: WindowRef[];
  /** The window a window row switches to. */
  window?: number;
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
  mode: 'search' | 'destinations' | 'recent';
  items: Suggestion[];
  notice: string | null;
  preselect: number | null;
  target: string | null;
  text: string;
  searchWith: string[];
}

export type DispatchOutcome =
  | { kind: 'opened'; destinationId: string }
  | { kind: 'armed'; destinationId: string }
  | { kind: 'launched'; appId: string }
  | { kind: 'settingOpened'; settingId: string }
  | { kind: 'fileOpened'; path: string }
  | { kind: 'ask'; query: string }
  | { kind: 'palette' }
  | { kind: 'unknownBang'; trigger: string }
  | { kind: 'empty' };

export function isSnapshot(value: unknown): value is Snapshot {
  if (typeof value !== 'object' || value === null) return false;
  return Array.isArray((value as Snapshot).destinations);
}

export type SyncPhase =
  | { kind: 'signedOut' }
  | { kind: 'browser' }
  | { kind: 'showRecovery'; recovery: string }
  | { kind: 'awaitingApproval'; words: string }
  | { kind: 'ready' };

export interface SyncDevice {
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

/** A browser profile bookmarks are read from. */
export interface BookmarkSource {
  /** e.g. chrome/Default */
  id: string;
  /** e.g. "Chrome · Zach" */
  label: string;
  count: number;
  enabled: boolean;
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
 * A hotkey and/or alias for one thing the bar can do. target: view:ai, scope:files,
 * app:<id>, dest:<id> or bookmark:<url>.
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

/** Live system stats for the bar's footer. */
export interface SystemStats {
  cpu: number;
  memoryUsed: number;
  memoryTotal: number;
  diskFree: number;
  diskTotal: number;
  netDown: number;
  netUp: number;
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
  user: string;
  path: string;
  command: string;
  /** The system needs it, so it can't be force quit. */
  protected: boolean;
  /** Finder, the Dock, Explorer: relaunched rather than quit. */
  relaunch: boolean;
  /** Its use can't be read without administrator rights. */
  locked: boolean;
}

export interface ResourcePoint {
  cpu: number;
  /** Share of memory in use, 0 to 1. */
  memory: number;
  down: number;
  up: number;
}

export interface ResourceSnapshot {
  totals: SystemStats;
  cores: number;
  /** The last minute, a point a second, oldest first. */
  history: ResourcePoint[];
  rows: ResourceRow[];
  processes: number;
  /** Seconds since the computer started. */
  uptime: number;
}

export type ResourceSort = 'cpu' | 'memory' | 'disk' | 'name';

/** The ChatGPT account Ask AI uses, when signed in with ChatGPT. */
export interface ChatgptAccount {
  email: string;
  name: string;
  /** False when sign-in worked but sharing the plan wasn't allowed. */
  plan: boolean;
}

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
