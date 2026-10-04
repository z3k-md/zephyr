export type SuggestKind = 'none' | 'google' | 'youtube' | 'wikipedia' | 'pubmed';

export interface Destination {
  id: string;
  name: string;
  triggers: string[];
  urlTemplate: string;
  suggest: SuggestKind;
  pinned: boolean;
  builtin: boolean;
  disabled: boolean;
}

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
}

export interface Suggestion {
  label: string;
  query: string;
  destinationId: string;
  kind: 'history' | 'remote' | 'destination';
  hint: string;
}

export interface SuggestResponse {
  mode: 'search' | 'destinations' | 'recent';
  items: Suggestion[];
  notice: string | null;
}

export type DispatchOutcome =
  | { kind: 'opened'; destinationId: string }
  | { kind: 'armed'; destinationId: string }
  | { kind: 'palette' }
  | { kind: 'unknownBang'; trigger: string }
  | { kind: 'empty' };

export function isSnapshot(value: unknown): value is Snapshot {
  if (typeof value !== 'object' || value === null) return false;
  return Array.isArray((value as Snapshot).destinations);
}
