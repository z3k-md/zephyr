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
}

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
    | 'view';
  hint: string;
  appId?: string;
  settingId?: string;
  path?: string;
  noteId?: string;
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
