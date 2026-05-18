import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Hotkey = 'right-option' | 'left-option' | 'right-command' | 'fn' | 'f5';
export type PermissionStatus = 'granted' | 'denied' | 'not_determined';
export type Route = 'home' | 'settings' | 'wizard' | 'model-manager';

export interface Settings {
  whisperModelPath: string | null;
  llmModelPath: string | null;
  llmCtxSize: number;
  whisperCoreMLDisable: boolean;
  hotkey: Hotkey;
  language: string;
  forcePasteboard: boolean;
  pasteDelayMs: number;
  launchAtLogin: boolean;
  uiLanguage: string;
  onboardingComplete: boolean;
  appVersion: string;
}

export type ModelState =
  | { kind: 'idle' }
  | { kind: 'loading'; whisper: boolean; llama: boolean }
  | { kind: 'ready'; whisper: boolean; llama: boolean }
  | { kind: 'reloading'; reason: string }
  | { kind: 'error'; reason: string };

export interface CatalogEntry {
  id: string;
  kind: 'stt' | 'llm';
  displayName: string;
  description: string;
  sizeBytes: number;
  filename: string;
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
  kind: 'info' | 'warn' | 'error';
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

export const lda = {
  getSettings: () => invoke<Settings>('get_settings'),
  updateSettings: (patch: Partial<Settings>) => invoke<Settings>('update_settings', { patch }),
  modelsCatalog: () => invoke<CatalogEntry[]>('models_catalog'),
  modelsListLocal: () => invoke<LocalModel[]>('models_list_local'),
  modelsDownload: (id: string) => invoke<void>('models_download', { id }),
  modelsCancelDownload: (id: string) => invoke<void>('models_cancel_download', { id }),
  checkAccessibility: () => invoke<PermissionStatus>('check_accessibility'),
  promptAccessibility: () => invoke<PermissionStatus>('prompt_accessibility'),
  checkMicrophone: () => invoke<PermissionStatus>('check_microphone'),
  testMic: () => invoke<number>('test_mic'),
  openWindow: (route: Route) => invoke<void>('open_window', { route }),
  closeWindow: () => invoke<void>('close_window'),
  completeWizard: () => invoke<void>('complete_wizard'),
  pickFile: (filters: FileFilter[]) => invoke<string | null>('pick_file', { filters }),
  checkForUpdates: () => invoke<UpdateInfo>('check_for_updates'),

  onModelState: (cb: (s: ModelState) => void): Promise<UnlistenFn> =>
    listen<ModelState>('model:state', (e) => cb(e.payload)),
  onRecordingState: (cb: (rec: boolean) => void): Promise<UnlistenFn> =>
    listen<boolean>('recording:state', (e) => cb(e.payload)),
  onAudioLevel: (cb: (level: number) => void): Promise<UnlistenFn> =>
    listen<number>('recording:level', (e) => cb(e.payload)),
  onDownloadProgress: (cb: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
    listen<DownloadProgress>('download:progress', (e) => cb(e.payload)),
  onToast: (cb: (t: Toast) => void): Promise<UnlistenFn> =>
    listen<Toast>('toast', (e) => cb(e.payload)),
};
