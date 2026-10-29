import { Channel, invoke } from '@tauri-apps/api/core';
import type {
  ClipDetail,
  ClipList,
  ClipboardSettings,
  DispatchOutcome,
  FileIndexStatus,
  Note,
  NoteSummary,
  Binding,
  FilePreview,
  MediaControl,
  NowPlaying,
  Forecast,
  Place,
  WeatherUnit,
  UpdateStatus,
  PowerAction,
  PowerPreview,
  ShellEvent,
  ShellInfo,
  ShellProgram,
  Snapshot,
  SyncPending,
  SyncStatus,
  ResourceSnapshot,
  ResourceSort,
  SystemStats,
  LaunchGroup,
  Permission,
  PermissionId,
  TypingBest,
  SuggestResponse,
  NetworkAddress,
  NetworkHistory,
  NetworkSummary,
  SpeedResult,
  ResourceDetail,
  CoreHeat,
  MemoryBreakdown,
} from './types';

export function getSnapshot(): Promise<Snapshot> {
  return invoke<Snapshot>('get_snapshot');
}

export function suggest(query: string, includeRemote: boolean): Promise<SuggestResponse> {
  return invoke<SuggestResponse>('suggest', { query, includeRemote });
}

/**
 * Opens an app, or switches to it when it's running; newWindow starts another one, and
 * privateWindow opens a browser in its private window.
 */
export function launchApp(
  appId: string,
  newWindow = false,
  privateWindow = false
): Promise<DispatchOutcome> {
  return invoke<DispatchOutcome>('launch_app', { appId, newWindow, private: privateWindow });
}

/** Switches to an open window. */
export function windowFocus(handle: number): Promise<void> {
  return invoke('window_focus', { handle });
}

/** Asks an open window to close, as its close button would. */
export function windowClose(handle: number): Promise<void> {
  return invoke('window_close', { handle });
}

/** Asks every window of an app to close; nothing is force-quit. */
export function appQuit(appId: string): Promise<void> {
  return invoke('app_quit', { appId });
}

/** Opens the connection to Google's suggestion service before the first keystroke. */
export function warmSuggestions(): Promise<void> {
  return invoke('warm_suggestions');
}

/** Adds an entry to the running bug capture's log (Ctrl+R). */
export function captureEvent(kind: string, data: unknown): Promise<void> {
  return invoke('capture_event', { kind, data });
}

export function captureRecording(): Promise<boolean> {
  return invoke<boolean>('capture_recording');
}

/** Ctrl+R: starts or stops a bug recording of the Zephyr window in front. */
export function captureToggle(): Promise<void> {
  return invoke('capture_toggle');
}

/** Ctrl+Shift+S: a bug snapshot of the Zephyr window in front, also put on the clipboard. */
export function captureSnapshot(): Promise<void> {
  return invoke('capture_snapshot');
}

/** Makes the snap grid's window before the first Ctrl+drag. */
export function prepareGrid(): Promise<void> {
  return invoke('prepare_grid');
}

/** Starts moving the bar. On a Mac the page drives the move: [x, y, scale] come back. */
export function startBarDrag(): Promise<[number, number, number] | null> {
  return invoke<[number, number, number] | null>('start_bar_drag');
}

/** Mac ⌘-drag: the bar's new top-left, in physical pixels. */
export function barMove(x: number, y: number): Promise<void> {
  return invoke('bar_move', { x, y });
}

/** Mac ⌘-drag ended: the bar lands on the guide it was near. */
export function barMoveEnd(): Promise<void> {
  return invoke('bar_move_end');
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

/** Does what Enter would with `query`; `interpret` off searches it as it is (a picked suggestion). */
export function dispatch(query: string, interpret: boolean): Promise<DispatchOutcome> {
  return invoke<DispatchOutcome>('dispatch', { query, interpret });
}

export function saveSettings(summonShortcut: string, launchAtStartup: boolean): Promise<Snapshot> {
  return invoke<Snapshot>('save_settings', { summonShortcut, launchAtStartup });
}

export function setBarHeight(height: number, width?: number): Promise<void> {
  return invoke('set_bar_height', { height, width: width ?? null });
}

export function hideBar(): Promise<void> {
  return invoke('hide_bar');
}

export function hideSettings(): Promise<void> {
  return invoke('hide_settings');
}

/** The page's own title strip: move, minimize, maximize or restore, or close its window. */
export function windowChrome(action: 'drag' | 'minimize' | 'maximize' | 'close'): Promise<void> {
  return invoke('window_chrome', { action });
}

export function openSettings(): Promise<void> {
  return invoke('open_settings');
}

export function checkForUpdates(): Promise<string> {
  return invoke<string>('check_for_updates');
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

/** Turns Zephyr's own shortcuts off while one is being recorded, and back on after. */
export function pauseShortcuts(paused: boolean): Promise<void> {
  return invoke('pause_shortcuts', { paused });
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

export function saveResumeSeconds(seconds: number): Promise<Snapshot> {
  return invoke<Snapshot>('save_resume_seconds', { seconds });
}

export function saveTheme(theme: string): Promise<Snapshot> {
  return invoke<Snapshot>('save_theme', { theme });
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

/** The system folder picker; null when cancelled. */
export function pickFolder(): Promise<string | null> {
  return invoke<string | null>('pick_folder');
}

export function setActiveMode(mode: string): Promise<void> {
  return invoke('set_active_mode', { mode });
}

/** From a note's window: opens `id` (a new note when null) in a window of its own. */
export function openNoteWindow(id: string | null): Promise<string> {
  return invoke<string>('open_note_window', { id });
}

/** From a note's window, once it has saved: closes that window; the note stays. */
export function closeNoteWindow(): Promise<void> {
  return invoke('close_note_window');
}

/** From a note's window whose save failed while closing: keeps it open. */
export function keepNoteWindow(): Promise<void> {
  return invoke('keep_note_window');
}

export function saveOpenBehavior(returnToMode: boolean): Promise<Snapshot> {
  return invoke<Snapshot>('save_open_behavior', { returnToMode });
}

export function permissionsStatus(): Promise<Permission[]> {
  return invoke<Permission[]>('permissions_status');
}

/** Allow on a permission: the system prompt, or the exact System Settings pane. */
export function permissionRequest(id: PermissionId): Promise<Permission[]> {
  return invoke<Permission[]>('permission_request', { id });
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

/** What typed text like "shutdown in 30m" would do, or null. */
export function powerPreview(query: string): Promise<PowerPreview | null> {
  return invoke<PowerPreview | null>('power_preview', { query });
}

/** Runs a power action now, or schedules it for `at` (unix seconds). */
export function powerRun(action: PowerAction, at: number | null): Promise<void> {
  return invoke('power_run', { action, at });
}

export function powerCancel(): Promise<void> {
  return invoke('power_cancel');
}

/** Opens a web address in the default browser and closes the bar. */
export function openUrl(url: string): Promise<void> {
  return invoke('open_url', { url });
}

/** A site's icon by address, as a data URL, or null. */
export function siteIconUrl(url: string): Promise<string | null> {
  return invoke<string | null>('site_icon_url', { url });
}

/** The last weather fetched; later ones arrive as `weather` events. */
export function weather(): Promise<Forecast | null> {
  return invoke<Forecast | null>('weather');
}

/** Cities matching typed text, for the weather place setting. */
export function weatherPlaces(text: string): Promise<Place[]> {
  return invoke<Place[]>('weather_places', { text });
}

/** Connects to the city search ahead of typing. */
export function weatherWarm(): Promise<void> {
  return invoke('weather_warm');
}

/** The weather for a city typed in the bar. */
export function weatherAt(text: string): Promise<Forecast | null> {
  return invoke<Forecast | null>('weather_at', { text });
}

export function saveWeatherPlace(place: Place | null): Promise<Snapshot> {
  return invoke<Snapshot>('save_weather_place', { place });
}

/** Celsius or Fahrenheit for the weather; null follows the region's own way. */
export function saveWeatherUnit(unit: WeatherUnit | null): Promise<Snapshot> {
  return invoke<Snapshot>('save_weather_unit', { unit });
}

/** What's playing now; later changes arrive as `nowplaying` events. */
export function nowPlaying(): Promise<NowPlaying | null> {
  return invoke<NowPlaying | null>('now_playing');
}

export function nowPlayingControl(control: MediaControl): Promise<void> {
  return invoke('now_playing_control', { control });
}

/** What's playing, a step louder or quieter in the volume mixer; the new level (0–1), or null
 *  where an app's own volume can't be set. */
export function nowPlayingVolume(step: number): Promise<number | null> {
  return invoke<number | null>('now_playing_volume', { step });
}

/** Switches to the app or browser tab playing; true when it could. */
export function nowPlayingFocus(): Promise<boolean> {
  return invoke<boolean>('now_playing_focus');
}

export function openOnboarding(): Promise<void> {
  return invoke('open_onboarding');
}

/** Marks first-run setup finished (or skipped) and closes its window. */
export function finishOnboarding(): Promise<Snapshot> {
  return invoke<Snapshot>('finish_onboarding');
}

export function saveOnboardingEveryLaunch(every: boolean): Promise<Snapshot> {
  return invoke<Snapshot>('save_onboarding_every_launch', { every });
}

/** Installed apps matching typed text, ranked as the bar ranks them: [id, name]. */
export function searchApps(text: string, limit: number): Promise<[string, string][]> {
  return invoke<[string, string][]>('search_apps', { text, limit });
}

/** A view was opened, for the empty bar's Views to follow what's used. */
export function recordViewOpen(view: string): Promise<void> {
  return invoke('record_view_open', { view });
}

export function saveLaunchGroups(groups: LaunchGroup[]): Promise<Snapshot> {
  return invoke<Snapshot>('save_launch_groups', { groups });
}

/** Opens everything in a launch group; the bar closes at once. */
export function launchGroup(id: string): Promise<void> {
  return invoke('launch_group', { id });
}

export function systemStats(): Promise<SystemStats> {
  return invoke<SystemStats>('system_stats');
}

export function resourcesSnapshot(
  sort: ResourceSort,
  filter: string,
  limit: number
): Promise<ResourceSnapshot> {
  return invoke<ResourceSnapshot>('resources_snapshot', { sort, filter, limit });
}

/** The view closed; the process table is let go a little later. */
export function resourcesClosed(): Promise<void> {
  return invoke('resources_closed');
}

export function resourcesQuit(name: string, pids: number[], force: boolean): Promise<void> {
  return invoke('resources_quit', { name, pids, force });
}

export function resourcesRelaunch(name: string, pids: number[]): Promise<void> {
  return invoke('resources_relaunch', { name, pids });
}

/** Brings the app forward and hides the bar; false when it has no windows. */
export function resourcesSwitch(pids: number[]): Promise<boolean> {
  return invoke<boolean>('resources_switch', { pids });
}

export function resourcesReveal(path: string): Promise<void> {
  return invoke('resources_reveal', { path });
}

export function saveShowStats(show: boolean): Promise<Snapshot> {
  return invoke<Snapshot>('save_show_stats', { show });
}

export function saveBugCapture(enabled: boolean): Promise<Snapshot> {
  return invoke<Snapshot>('save_bug_capture', { enabled });
}

export function revealCaptures(): Promise<void> {
  return invoke('reveal_captures');
}

/** Lets file search into Desktop, Documents and Downloads (macOS asks for each). */
export function allowProtectedFolders(): Promise<Snapshot> {
  return invoke<Snapshot>('allow_protected_folders');
}

/** What the update card shows now; later changes arrive as `update-status` events. */
export function updateStatus(): Promise<UpdateStatus | null> {
  return invoke<UpdateStatus | null>('update_status');
}

export function updateInstallNow(): Promise<void> {
  return invoke('update_install_now');
}

export function updateLater(): Promise<void> {
  return invoke('update_later');
}

/** A downloaded update put off with Later, by version; later changes arrive as
 *  `update-waiting` events. */
export function updateWaiting(): Promise<string | null> {
  return invoke<string | null>('update_waiting');
}

/** Dev builds only: plays the update card through with a made-up update. */
export function rehearseUpdate(): Promise<void> {
  return invoke('rehearse_update');
}

/** Saves a hotkey and alias for one target; both empty removes it. */
export function saveBinding(binding: Binding): Promise<Snapshot> {
  return invoke<Snapshot>('save_binding', { binding });
}

/** Targets whose hotkey another app already holds. */
export function bindingFailures(): Promise<string[]> {
  return invoke<string[]>('binding_failures');
}

/** From the bar: opens Settings > Shortcuts on this target. */
export function editBinding(target: string, label: string): Promise<void> {
  return invoke('edit_binding', { target, label });
}

/** For Settings: the target the bar asked about, once. */
export function takePendingBinding(): Promise<{ target: string; label: string } | null> {
  return invoke('take_pending_binding');
}

/** Gets a file result or clipboard item ready to drag out; the bar stays up meanwhile. */
export function dragPrepare(source: {
  path?: string;
  clip?: number;
}): Promise<{ files: string[]; icon: string }> {
  return invoke('drag_prepare', { path: source.path ?? null, clip: source.clip ?? null });
}

export function dragFinished(dropped: boolean): Promise<void> {
  return invoke('drag_finished', { dropped });
}

/** A look at a file result without opening it. */
export function filePreview(path: string): Promise<FilePreview> {
  return invoke<FilePreview>('file_preview', { path });
}

/** macOS: Quick Look on a file result. */
export function quickLook(path: string): Promise<void> {
  return invoke('quick_look', { path });
}

/** Windows: start from a logon task right at sign-in instead of the delayed startup list. */
export function saveInstantStartup(enabled: boolean): Promise<Snapshot> {
  return invoke<Snapshot>('save_instant_startup', { enabled });
}

/** The connection's grade and numbers over 15 minutes; null before the monitor starts. */
export function networkSummary(): Promise<NetworkSummary | null> {
  return invoke<NetworkSummary | null>('network_summary');
}

/** The last 15 minutes in detail, for the Network view. */
export function networkHistory(): Promise<NetworkHistory | null> {
  return invoke<NetworkHistory | null>('network_history');
}

/** The address the internet sees; asked only while the IP card or Network view shows. */
export function networkPublicIp(): Promise<string | null> {
  return invoke<string | null>('network_public_ip');
}

export function networkAddresses(): Promise<NetworkAddress[]> {
  return invoke<NetworkAddress[]>('network_addresses');
}

export function saveWatchConnection(watch: boolean): Promise<Snapshot> {
  return invoke<Snapshot>('save_watch_connection', { watch });
}

/** Runs a speed test; progress arrives as "speed-test" events to the bar. */
export function speedTest(): Promise<SpeedResult> {
  return invoke<SpeedResult>('speed_test');
}

export function speedTestCancel(): Promise<void> {
  return invoke('speed_test_cancel');
}

export function resourcesDetail(key: string, pids: number[]): Promise<ResourceDetail | null> {
  return invoke<ResourceDetail | null>('resources_detail', { key, pids });
}

export function resourcesCores(): Promise<CoreHeat> {
  return invoke<CoreHeat>('resources_cores');
}

export function resourcesMemory(): Promise<MemoryBreakdown | null> {
  return invoke<MemoryBreakdown | null>('resources_memory');
}

/** Turns the Windows CPU temperature reading on or off; Windows asks for permission. */
export function saveCpuTemp(enabled: boolean): Promise<Snapshot> {
  return invoke<Snapshot>('save_cpu_temp', { enabled });
}

export function cpuTempStatus(): Promise<{ pawnio: boolean; celsius: number | null }> {
  return invoke('cpu_temp_status');
}
