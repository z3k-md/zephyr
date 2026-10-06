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
  kind: 'history' | 'remote' | 'destination' | 'app' | 'setting' | 'file' | 'answer';
  hint: string;
  appId?: string;
  settingId?: string;
  path?: string;
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
