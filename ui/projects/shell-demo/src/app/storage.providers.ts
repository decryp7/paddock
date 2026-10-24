import { MemoryStorageBackend, provideShellStorage, TauriStorageBackend, type StorageBackend } from '@decryp7/paddock-ui/shell/storage';
import { tauriIpc } from './demo-ipc';

let memory: MemoryStorageBackend | null = null;

/**
 * What the app keeps over Tauri in the desktop build; in the browser, one in-memory engine per page (the settings window and its
 * in-page dialogs share it, as windows share the real engine). Its phases pace at a visible speed. The sizes are fictional.
 */
export function demoStorageBackend(): StorageBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriStorageBackend(ipc);
  memory ??= new MemoryStorageBackend({ schedule: (step) => void setTimeout(step, 400) });
  return memory;
}

/** What the Storage page boots with (loaded with the page, never in the main window's first paint). */
export const STORAGE_PROVIDERS = [provideShellStorage({ backend: demoStorageBackend })];
