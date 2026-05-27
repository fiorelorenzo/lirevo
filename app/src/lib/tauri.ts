import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Hotkey = 'right-option' | 'left-option' | 'right-command' | 'fn' | 'f5';
export type PermissionStatus = 'granted' | 'denied' | 'not_determined';
export type Route = 'home' | 'settings' | 'wizard';

export interface Settings {
  /** M4: catalog id of the STT model to load. `null` falls back to the
   * backend's `default_model_id()`. Replaces the pre-M4 `whisperModelPath`
   * for STT selection; the legacy field is kept for backwards-compatible
   * deserialization but is no longer read by the loader. */
  sttModelId: string | null;
  whisperModelPath: string | null;
  llmModelPath: string | null;
  llmCtxSize: number;
  whisperCoreMLDisable: boolean;
  hotkey: Hotkey;
  language: string;
  inputDeviceName: string | null;
  forcePasteboard: boolean;
  pasteDelayMs: number;
  launchAtLogin: boolean;
  launchMinimized: boolean;
  stayRunningOnWindowClose: boolean;
  keepModelsWarm: boolean;
  uiLanguage: string;
  onboardingComplete: boolean;
  appVersion: string;
}

export type ModelState =
  | { kind: 'idle' }
  | { kind: 'loading'; stt: boolean; llama: boolean }
  | { kind: 'ready'; stt: boolean; llama: boolean }
  | { kind: 'reloading'; reason: string }
  | { kind: 'error'; reason: string };

export interface ModelScores {
  /** 0-100, higher is better. */
  quality: number;
  latency: number;
  ram: number;
  /** Unweighted mean of the three axes. */
  compositeEqual: number;
  /** 0.5·quality + 0.3·latency + 0.2·ram. UI default. */
  compositeWeighted: number;
  rawChrfMean: number;
  rawWarmP50Ms?: number;
  rawPeakRssKb?: number;
  nCells: number;
}

export interface CatalogEntry {
  id: string;
  kind: 'stt' | 'llm';
  displayName: string;
  description: string;
  sizeBytes: number;
  filename: string;
  /** Bake-off scores. Always undefined for STT entries. */
  scores?: ModelScores;
  /** Marked by `lirevo-eval bless` on the weighted-composite winner. */
  recommended: boolean;
}

export interface LocalModel {
  id: string;
  kind: 'stt' | 'llm';
  path: string;
  sizeBytes: number;
  inCatalog: boolean;
}

export interface DownloadProgress {
  id: string;
  state: 'queued' | 'downloading' | 'verifying' | 'complete' | 'error' | 'cancelled';
  bytesReceived: number;
  bytesTotal: number;
  errorMessage?: string;
}

export interface Toast {
  kind: 'info' | 'warn' | 'error' | 'success';
  message: string;
}

export interface FileFilter {
  name: string;
  extensions: string[];
}

export interface UpdateInfo {
  available: boolean;
  version: string | null;
}

export interface TestMicResult {
  peak: number;
  sampleCount: number;
  deviceLabel: string;
  detected: boolean;
  cancelled: boolean;
  /** Device returned samples but every level was exactly zero ≥3s. */
  deviceSilent: boolean;
}

export interface InputDeviceEntry {
  name: string;
  isDefault: boolean;
}

/** Live partial-transcript update emitted by the streaming STT worker.
 *  `text` is authoritative cumulative; `delta` is a hint of the tail
 *  added since the previous event (may shrink/rewrite). `isFinal` is
 *  true only on the very last event of a dictation. */
export interface PartialTranscript {
  text: string;
  delta: string;
  isFinal: boolean;
}

export const lda = {
  getSettings: () => invoke<Settings>('get_settings'),
  updateSettings: (patch: Partial<Settings>) => invoke<Settings>('update_settings', { patch }),
  modelsCatalog: () => invoke<CatalogEntry[]>('models_catalog'),
  modelsListLocal: () => invoke<LocalModel[]>('models_list_local'),
  modelsDownload: (id: string) => invoke<void>('models_download', { id }),
  modelsCancelDownload: (id: string) => invoke<void>('models_cancel_download', { id }),
  modelsDelete: (id: string) => invoke<void>('models_delete', { id }),
  getModelState: () => invoke<ModelState>('get_model_state'),
  checkAccessibility: () => invoke<PermissionStatus>('check_accessibility'),
  promptAccessibility: () => invoke<PermissionStatus>('prompt_accessibility'),
  checkMicrophone: () => invoke<PermissionStatus>('check_microphone'),
  promptMicrophone: () => invoke<PermissionStatus>('prompt_microphone'),
  openSystemSettingsMicrophone: () => invoke<void>('open_system_settings_microphone'),
  openSystemSettingsAccessibility: () => invoke<void>('open_system_settings_accessibility'),
  retryHotkeyInstall: () => invoke<void>('retry_hotkey_install'),
  testMic: (deviceName: string | null) =>
    invoke<TestMicResult>('test_mic', { deviceName }),
  cancelTestMic: () => invoke<void>('cancel_test_mic'),
  listInputDevices: () => invoke<InputDeviceEntry[]>('list_input_devices'),
  openWindow: (route: Route) => invoke<void>('open_window', { route }),
  closeWindow: () => invoke<void>('close_window'),
  completeWizard: () => invoke<void>('complete_wizard'),
  pickFile: (filters: FileFilter[]) => invoke<string | null>('pick_file', { filters }),
  checkForUpdates: () => invoke<UpdateInfo>('check_for_updates'),

  onModelState: (cb: (s: ModelState) => void): Promise<UnlistenFn> =>
    listen<ModelState>('model:state', (e) => cb(e.payload)),
  onSettingsChanged: (cb: (s: Settings) => void): Promise<UnlistenFn> =>
    listen<Settings>('settings:changed', (e) => cb(e.payload)),
  onRecordingState: (cb: (rec: boolean) => void): Promise<UnlistenFn> =>
    listen<boolean>('recording:state', (e) => cb(e.payload)),
  onAudioLevel: (cb: (level: number) => void): Promise<UnlistenFn> =>
    listen<number>('recording:level', (e) => cb(e.payload)),
  onPartialTranscript: (cb: (p: PartialTranscript) => void): Promise<UnlistenFn> =>
    listen<PartialTranscript>('recording:partial_transcript', (e) => cb(e.payload)),
  onDownloadProgress: (cb: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
    listen<DownloadProgress>('download:progress', (e) => cb(e.payload)),
  onToast: (cb: (t: Toast) => void): Promise<UnlistenFn> =>
    listen<Toast>('toast', (e) => cb(e.payload)),
};
