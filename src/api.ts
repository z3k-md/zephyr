import { Channel, invoke } from '@tauri-apps/api/core';
import type {
  AiEvent,
  ClaudeApproval,
  ClaudeCliStatus,
  ClaudeJob,
  ClaudeSettings,
  ClipDetail,
  ClipList,
  ClipboardSettings,
  AiPreset,
  AiSettings,
  Destination,
  DispatchOutcome,
  FileIndexStatus,
  LocalServer,
  Note,
  NoteSummary,
  ShellEvent,
  ShellInfo,
  ShellProgram,
  Snapshot,
  SyncPending,
  SyncStatus,
  TypingBest,
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

/** Opens the connection to a destination's suggestion service before the first keystroke. */
export function warmSuggestions(destinationId: string): Promise<void> {
  return invoke('warm_suggestions', { destinationId });
}

/** Adds an entry to the running bug capture's log (Ctrl+Alt+R in dev builds). */
export function captureEvent(kind: string, data: unknown): Promise<void> {
  return invoke('capture_event', { kind, data });
}

export function captureRecording(): Promise<boolean> {
  return invoke<boolean>('capture_recording');
}

/** Moves the bar with the mouse until the button is released (Ctrl+drag). */
export function startBarDrag(): Promise<void> {
  return invoke('start_bar_drag');
}

/** An app's icon as a PNG data URL at `size` pixels, or null when it has none to offer. */
export function appIcon(appId: string, size: number): Promise<string | null> {
  return invoke<string | null>('app_icon', { appId, size });
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

export function setBarHeight(height: number, width?: number): Promise<void> {
  return invoke('set_bar_height', { height, width: width ?? null });
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

export function aiAsk(
  question: string,
  onEvent: (event: AiEvent) => void,
  history: { question: string; answer: string }[] = []
): Promise<void> {
  const channel = new Channel<AiEvent>();
  channel.onmessage = onEvent;
  return invoke('ai_ask', { question, history, onEvent: channel });
}

export function aiCancel(): Promise<void> {
  return invoke('ai_cancel');
}

export function saveAiSettings(settings: AiSettings): Promise<Snapshot> {
  return invoke<Snapshot>('save_ai_settings', { settings });
}

export function aiPresets(): Promise<AiPreset[]> {
  return invoke<AiPreset[]>('ai_presets');
}

export function aiSetKey(provider: string, key: string | null): Promise<void> {
  return invoke('ai_set_key', { provider, key });
}

export function aiHasKey(provider: string): Promise<boolean> {
  return invoke<boolean>('ai_has_key', { provider });
}

export function aiModels(settings: AiSettings): Promise<string[]> {
  return invoke<string[]>('ai_models', { settings });
}

export function aiDetectLocal(): Promise<LocalServer[]> {
  return invoke<LocalServer[]>('ai_detect_local');
}

export function resolveUrl(query: string, destinationId: string): Promise<string> {
  return invoke<string>('resolve_url', { query, destinationId });
}

export function exportDestinations(): Promise<string> {
  return invoke<string>('export_destinations');
}

export function importDestinations(json: string): Promise<Snapshot> {
  return invoke<Snapshot>('import_destinations', { json });
}

export function clipList(query: string, filter: string): Promise<ClipList> {
  return invoke<ClipList>('clip_list', { query, filter });
}

export function clipDetail(id: number): Promise<ClipDetail> {
  return invoke<ClipDetail>('clip_detail', { id });
}

/** Resolves to 'pasted', or 'needsPermission' when it could only copy. */
export function clipPaste(id: number, plain: boolean): Promise<'pasted' | 'needsPermission'> {
  return invoke('clip_paste', { id, plain });
}

export function clipCopy(id: number): Promise<void> {
  return invoke('clip_copy', { id });
}

export function clipPin(id: number, pinned: boolean): Promise<void> {
  return invoke('clip_pin', { id, pinned });
}

export function clipDelete(id: number): Promise<void> {
  return invoke('clip_delete', { id });
}

export function clipClear(pinnedToo: boolean): Promise<number> {
  return invoke<number>('clip_clear', { pinnedToo });
}

export function clipCanPaste(): Promise<boolean> {
  return invoke<boolean>('clip_can_paste');
}

export function clipRequestPastePermission(): Promise<boolean> {
  return invoke<boolean>('clip_request_paste_permission');
}

export function clipStats(): Promise<Record<string, number>> {
  return invoke('clip_stats');
}

export function saveClipboardSettings(settings: ClipboardSettings): Promise<Snapshot> {
  return invoke<Snapshot>('save_clipboard_settings', { settings });
}

export function openClipboard(): Promise<void> {
  return invoke('open_clipboard');
}

export function notesList(query: string): Promise<NoteSummary[]> {
  return invoke<NoteSummary[]>('notes_list', { query });
}

export function noteGet(id: string): Promise<Note> {
  return invoke<Note>('note_get', { id });
}

export function noteCreate(body: string): Promise<string> {
  return invoke<string>('note_create', { body });
}

export function noteSave(id: string, body: string): Promise<number> {
  return invoke<number>('note_save', { id, body });
}

export function noteDelete(id: string): Promise<void> {
  return invoke('note_delete', { id });
}

export function revealNotes(): Promise<void> {
  return invoke('reveal_notes');
}

export function openNotes(id?: string, body?: string): Promise<void> {
  return invoke('open_notes', { id: id ?? null, body: body ?? null });
}

export function saveNotesShortcut(shortcut: string): Promise<Snapshot> {
  return invoke<Snapshot>('save_notes_shortcut', { shortcut });
}

/** Shows this settings or notes window once its first frame has painted. */
export function pageReady(): void {
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      void invoke('page_ready').catch(() => undefined);
    })
  );
}

/** Resolves true when the result is a new personal best for its mode. */
export function saveTypingResult(result: TypingBest): Promise<boolean> {
  return invoke<boolean>('save_typing_result', { result });
}

export function claudeJobs(): Promise<ClaudeJob[]> {
  return invoke<ClaudeJob[]>('claude_jobs');
}

export function claudeApprovals(): Promise<ClaudeApproval[]> {
  return invoke<ClaudeApproval[]>('claude_approvals');
}

export function claudeSubmit(input: string, projectId?: string): Promise<string> {
  return invoke<string>('claude_submit', { input, projectId: projectId ?? null });
}

export function claudeFollowUp(jobId: string, prompt: string): Promise<void> {
  return invoke('claude_follow_up', { jobId, prompt });
}

export function claudeCancel(jobId: string): Promise<void> {
  return invoke('claude_cancel', { jobId });
}

export function claudeRemove(jobId: string): Promise<void> {
  return invoke('claude_remove', { jobId });
}

export function claudeOpenTerminal(jobId: string): Promise<void> {
  return invoke('claude_open_terminal', { jobId });
}

export function claudeAnswer(
  approvalId: string,
  decision: 'allow' | 'always' | 'deny',
  reason?: string
): Promise<void> {
  return invoke('claude_answer', { approvalId, decision, reason: reason ?? null });
}

export function claudeStatus(): Promise<ClaudeCliStatus> {
  return invoke<ClaudeCliStatus>('claude_status');
}

export function saveClaudeSettings(settings: ClaudeSettings): Promise<Snapshot> {
  return invoke<Snapshot>('save_claude_settings', { settings });
}

export function saveResumeSeconds(seconds: number): Promise<Snapshot> {
  return invoke<Snapshot>('save_resume_seconds', { seconds });
}

/** Runs a shell command in `dir` (home when null); the run's id arrives in its `started` event. */
export function shellRun(
  command: string,
  dir: string | null,
  onEvent: (event: ShellEvent) => void
): Promise<void> {
  const channel = new Channel<ShellEvent>();
  channel.onmessage = onEvent;
  return invoke('shell_run', { command, dir, onEvent: channel });
}

export function shellStop(id: number): Promise<void> {
  return invoke('shell_stop', { id });
}

/** Opens the command in a terminal window in `dir` and closes the bar. */
export function shellOpenTerminal(command: string, dir: string | null): Promise<void> {
  return invoke('shell_open_terminal', { command, dir });
}

export function shellInfo(): Promise<ShellInfo> {
  return invoke<ShellInfo>('shell_info');
}

export function saveShellProgram(program: ShellProgram): Promise<Snapshot> {
  return invoke<Snapshot>('save_shell_program', { program });
}

export function saveShellTerminal(terminal: 'auto' | 'console'): Promise<Snapshot> {
  return invoke<Snapshot>('save_shell_terminal', { terminal });
}

export function clearShellHistory(): Promise<Snapshot> {
  return invoke<Snapshot>('clear_shell_history');
}

/** The system folder picker; null when cancelled. */
export function pickFolder(): Promise<string | null> {
  return invoke<string | null>('pick_folder');
}

export function setActiveMode(mode: string): Promise<void> {
  return invoke('set_active_mode', { mode });
}

export function leaveNotes(): Promise<void> {
  return invoke('leave_notes');
}

export function saveOpenBehavior(
  returnToMode: boolean,
  routeCmd: string,
  routeAlt: string
): Promise<Snapshot> {
  return invoke<Snapshot>('save_open_behavior', { returnToMode, routeCmd, routeAlt });
}

export function syncStatus(): Promise<SyncStatus> {
  return invoke<SyncStatus>('sync_status');
}

/** Opens Google sign-in in the browser; resolves once it comes back and sync has settled. */
export function syncGoogleSignIn(): Promise<void> {
  return invoke('sync_google_sign_in');
}

export function syncCancelSignIn(): Promise<void> {
  return invoke('sync_cancel_sign_in');
}

export function syncRecoverySaved(): Promise<void> {
  return invoke('sync_recovery_saved');
}

export function syncUseRecovery(key: string): Promise<void> {
  return invoke('sync_use_recovery', { key });
}

export function syncPoll(): Promise<void> {
  return invoke('sync_poll');
}

export function syncPending(): Promise<SyncPending[]> {
  return invoke<SyncPending[]>('sync_pending');
}

export function syncApprove(deviceId: string): Promise<void> {
  return invoke('sync_approve', { deviceId });
}

export function syncRevoke(deviceId: string): Promise<void> {
  return invoke('sync_revoke', { deviceId });
}

export function syncRecoveryKey(): Promise<string> {
  return invoke<string>('sync_recovery_key');
}

export function syncSignOut(): Promise<void> {
  return invoke('sync_sign_out');
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
