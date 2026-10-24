import { inject, Injectable } from '@angular/core';
import {
  MemorySettingsBackend,
  SETTINGS_OVERVIEW_FIXTURE,
  SETTINGS_PROBLEM_FIXTURE,
  TauriSettingsBackend,
  type SettingsBackend,
} from '@decryp7/paddock-ui/shell/settings';
import { DemoHealthEngine } from './demo-health';
import { tauriIpc } from './demo-ipc';

/**
 * The demo's stand-in for `paddock-settings` (the browser build has no Rust): the sections Rust wrote into the fixtures
 * (Appearance, General, Storage: every field kind), held in memory. `?settings=damaged` plays a damaged settings file at
 * start-up: defaults, the problem in the Settings window and one notification, as `paddock_settings::tauri::manage` does.
 */
@Injectable({ providedIn: 'root' })
export class DemoSettingsEngine {
  private readonly damaged = typeof location !== 'undefined' && new URLSearchParams(location.search).get('settings') === 'damaged';
  readonly backend = new MemorySettingsBackend(SETTINGS_OVERVIEW_FIXTURE, { problem: this.damaged ? SETTINGS_PROBLEM_FIXTURE : null });

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
