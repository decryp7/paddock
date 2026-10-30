import {
  MemorySettingsBackend,
  SETTINGS_OVERVIEW_FIXTURE,
  TauriSettingsBackend,
  type SetResult,
  type SettingsBackend,
  type SettingsChanges,
  type SettingsOverview,
} from '@decryp7/paddock-ui/shell/settings';
import { tauriIpc } from './ipc';

/** Where the browser build keeps the stand-in engine's values (the stand-in for `settings.json`). */
const STORE_KEY = 'pc.settings';

/**
 * The in-memory engine of the browser build, kept in localStorage so a reload starts from the user's choice (the real engine
 * keeps `settings.json`). Storage that is unavailable only costs the persistence.
 */
class PersistentMemorySettings extends MemorySettingsBackend {
  constructor(overview: SettingsOverview) {
    super(overview);
    for (const [section, values] of Object.entries(read())) void super.set(section, values).catch(() => undefined);
  }

  override async set(section: string, changes: SettingsChanges): Promise<SetResult> {
    const result = await super.set(section, changes);
    if (result.applied) this.keep();
    return result;
  }

  private keep(): void {
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify({ 'pd.appearance': this.values('pd.appearance') ?? {} }));
    } catch {
      /* storage unavailable: this page keeps its values in memory */
    }
  }
}

function read(): Record<string, SettingsChanges> {
  try {
    const v = JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}') as unknown;
    return typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, SettingsChanges>) : {};
  } catch {
    return {};
  }
}

let memory: PersistentMemorySettings | null = null;

/**
 * This window's settings backend: Tauri's in the desktop app; in the browser an in-memory engine with the shell's Appearance
 * section only (Pitcrew's own settings are the service's configuration, edited on its own pages).
 */
export function settingsBackend(): SettingsBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriSettingsBackend(ipc);
  memory ??= new PersistentMemorySettings({
    ...SETTINGS_OVERVIEW_FIXTURE,
    sections: SETTINGS_OVERVIEW_FIXTURE.sections.filter((s) => s.id === 'pd.appearance'),
  });
  return memory;
}
