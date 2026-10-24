import { inject, Injectable } from '@angular/core';
import {
  MemorySettingsBackend,
  type MemorySettingsOptions,
  type SettingsChanges,
  type SettingsOverview,
  SETTINGS_OVERVIEW_FIXTURE,
  SETTINGS_PROBLEM_FIXTURE,
  TauriSettingsBackend,
  type SettingsBackend,
  type SetResult,
} from '@decryp7/paddock-ui/shell/settings';
import { LOGGING_SECTION_FIXTURE } from '@decryp7/paddock-ui/shell/diagnostics';
import { DemoHealthEngine } from './demo-health';
import { tauriIpc } from './demo-ipc';

/** Where the browser build keeps the stand-in engine's values, and the channel its windows share. */
const STORE_KEY = 'sd.settings';

/**
 * The in-memory engine shared by the browser windows of the origin, as `paddock-settings` is shared by an app's windows:
 * a change made in one is applied in every other (BroadcastChannel, the stand-in for `paddock://settings` to every
 * window) and kept in localStorage (the stand-in for `settings.json`), so a reload starts from it.
 */
class SharedMemorySettingsBackend extends MemorySettingsBackend {
  private readonly channel: BroadcastChannel | null = typeof BroadcastChannel === 'function' ? new BroadcastChannel(STORE_KEY) : null;

  constructor(overview: SettingsOverview, options: MemorySettingsOptions) {
    super(overview, options);
    for (const [section, values] of Object.entries(read())) void super.set(section, values).catch(() => undefined);
    this.channel?.addEventListener('message', (e: MessageEvent<{ section: string; changes: SettingsChanges }>) => {
      void super.set(e.data.section, e.data.changes).then(() => this.keep()).catch(() => undefined);
    });
  }

  override async set(section: string, changes: SettingsChanges): Promise<SetResult> {
    const result = await super.set(section, changes);
    if (result.applied) {
      this.keep();
      this.channel?.postMessage({ section, changes });
    }
    return result;
  }

  private keep(): void {
    try {
      const sections = this.overviewNow().map((s) => [s.id, s.values]);
      localStorage.setItem(STORE_KEY, JSON.stringify(Object.fromEntries(sections)));
    } catch {
      /* storage unavailable: this window keeps its values in memory */
    }
  }

  private overviewNow(): { id: string; values: Record<string, unknown> }[] {
    return ['pd.appearance', 'sd.general', 'sd.storage', 'pd.logging'].map((id) => ({ id, values: this.values(id) ?? {} }));
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

/**
 * The demo's stand-in for `paddock-settings` (the browser build has no Rust): the sections Rust wrote into the fixtures
 * (Appearance, General, Storage: every field kind), held in memory. `?settings=damaged` plays a damaged settings file at
 * start-up: defaults, the problem in the Settings window and one notification, as `paddock_settings::tauri::manage` does.
 */
@Injectable({ providedIn: 'root' })
export class DemoSettingsEngine {
  private readonly damaged = typeof location !== 'undefined' && new URLSearchParams(location.search).get('settings') === 'damaged';
  readonly backend = new SharedMemorySettingsBackend(
    // The shell's Logging section (`paddock-diag`, written by Rust into its fixture) beside the sections of the settings fixture.
    { ...SETTINGS_OVERVIEW_FIXTURE, sections: [...SETTINGS_OVERVIEW_FIXTURE.sections, LOGGING_SECTION_FIXTURE] },
    { problem: this.damaged ? SETTINGS_PROBLEM_FIXTURE : null },
  );

  constructor() {
    if (!this.damaged) return;
    inject(DemoHealthEngine).centre.post({
      cause: 'settings:file',
      source: 'settings',
      severity: 'warning',
      title: 'Settings were reset to defaults',
      body: SETTINGS_PROBLEM_FIXTURE.message,
      primary: { type: 'openSettings' },
    });
  }
}

/** This window's settings backend: Tauri's in the desktop build, the demo engine in the browser. */
export function demoSettingsBackend(): SettingsBackend {
  const ipc = tauriIpc();
  return ipc ? new TauriSettingsBackend(ipc) : inject(DemoSettingsEngine).backend;
}
