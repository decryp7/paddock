import { MemorySettingsBackend, SETTINGS_OVERVIEW_FIXTURE, TauriSettingsBackend, type SettingsBackend } from '@decryp7/paddock-ui/shell/settings';
import { tauriIpc } from './ipc';

let memory: MemorySettingsBackend | null = null;

/**
 * This window's settings backend: Tauri's in the desktop app; in the browser an in-memory engine with the shell's Appearance
 * section only (Pitcrew's own settings are the service's configuration, edited on its own pages).
 */
export function settingsBackend(): SettingsBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriSettingsBackend(ipc);
  memory ??= new MemorySettingsBackend({
    ...SETTINGS_OVERVIEW_FIXTURE,
    sections: SETTINGS_OVERVIEW_FIXTURE.sections.filter((s) => s.id === 'pd.appearance'),
  });
  return memory;
}
