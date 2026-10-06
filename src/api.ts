import { invoke } from '@tauri-apps/api/core';
import type {
  Destination,
  DispatchOutcome,
  FileIndexStatus,
  Snapshot,
  SuggestResponse,
} from './types';

export function getSnapshot(): Promise<Snapshot> {
  return invoke<Snapshot>('get_snapshot');
}

export function suggest(
  query: string,
  destinationId: string,
  includeRemote: boolean
): Promise<SuggestResponse> {
  return invoke<SuggestResponse>('suggest', { query, destinationId, includeRemote });
}

export function launchApp(appId: string): Promise<DispatchOutcome> {
  return invoke<DispatchOutcome>('launch_app', { appId });
}

export function openSetting(settingId: string): Promise<DispatchOutcome> {
  return invoke<DispatchOutcome>('open_setting', { settingId });
}

export function openFile(path: string): Promise<DispatchOutcome> {
  return invoke<DispatchOutcome>('open_file', { path });
}

export function revealFile(path: string): Promise<void> {
  return invoke('reveal_file', { path });
}

export function fileIndexStatus(): Promise<FileIndexStatus> {
  return invoke<FileIndexStatus>('file_index_status');
}

export function rebuildFileIndex(): Promise<void> {
  return invoke('rebuild_file_index');
}

export function saveFileFolders(roots: string[] | null, excludes: string[]): Promise<Snapshot> {
  return invoke<Snapshot>('save_file_folders', { roots, excludes });
}

export function dispatch(
  query: string,
  destinationId: string,
  interpret: boolean
): Promise<DispatchOutcome> {
  return invoke<DispatchOutcome>('dispatch', { query, destinationId, interpret });
}

export function saveSettings(
  summonShortcut: string,
  launchAtStartup: boolean,
  defaultDestinationId: string
): Promise<Snapshot> {
  return invoke<Snapshot>('save_settings', {
    summonShortcut,
    launchAtStartup,
    defaultDestinationId,
  });
}

export function saveDestination(destination: Destination): Promise<Snapshot> {
  return invoke<Snapshot>('save_destination', { destination });
}

export function removeDestination(id: string): Promise<Snapshot> {
  return invoke<Snapshot>('remove_destination', { id });
}

export function moveDestination(id: string, delta: number): Promise<Snapshot> {
  return invoke<Snapshot>('move_destination', { id, delta });
}

export function clearHistory(): Promise<Snapshot> {
  return invoke<Snapshot>('clear_history');
}

export function setBarHeight(height: number): Promise<void> {
  return invoke('set_bar_height', { height });
}

export function hideBar(): Promise<void> {
  return invoke('hide_bar');
}

export function showBar(): Promise<void> {
  return invoke('show_bar');
}

export function openSettings(): Promise<void> {
  return invoke('open_settings');
}

export function checkForUpdates(): Promise<string> {
  return invoke<string>('check_for_updates');
}

export function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return 'Something went wrong';
}

const SHORTCUT_LABELS: Record<string, string> = {
  ctrl: 'Ctrl',
  control: 'Ctrl',
  alt: 'Alt',
  option: 'Alt',
  shift: 'Shift',
  super: 'Command',
  command: 'Command',
  cmd: 'Command',
  meta: 'Command',
  space: 'Space',
};

export function formatShortcut(shortcut: string): string {
  return shortcut
    .split('+')
    .filter((part) => part.length > 0)
    .map(
      (part) => SHORTCUT_LABELS[part.toLowerCase()] ?? part.charAt(0).toUpperCase() + part.slice(1)
    )
    .join('+');
}
