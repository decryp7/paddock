import { inject, Injectable } from '@angular/core';
import {
  MemoryAboutBackend,
  MemoryUpdatesBackend,
  TauriAboutBackend,
  TauriUpdatesBackend,
  type AboutBackend,
  type UpdatesBackend,
} from '@decryp7/paddock-ui/shell/about';
import { MemoryJobsBackend, TauriJobsBackend, type JobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { MemoryNotificationCentre, MemoryServiceHealth, TauriNotificationsBackend, TauriServiceHealthBackend, type Cause } from '@decryp7/paddock-ui/shell/notify';
import { MemorySettingsBackend, SETTINGS_OVERVIEW_FIXTURE, TauriSettingsBackend, type SettingsBackend } from '@decryp7/paddock-ui/shell/settings';
import { MockJobs } from '../core/mock/mock-jobs';
import { tauriIpc } from './ipc';

const cause = (message: string, hint: string, detail: string): Cause => ({ message, hint, detail, code: null, raiseCap: null, settings: null });

/**
 * The browser build's stand-ins for the shell's Rust parts (the browser has no Rust): service health with Spotter's
 * three services, the notification centre, jobs (the mock backend's operations play through them), settings and
 * About/updates. The shell's UI code is the same as in the app; only the backends are in memory. One instance per page,
 * as the real engine is one for all windows.
 */
@Injectable({ providedIn: 'root' })
export class BrowserEngines {
  readonly centre = new MemoryNotificationCentre();
  readonly health = new MemoryServiceHealth(
    [
      { id: 'sp.scheduler', label: 'the watch scheduler' },
      { id: 'sp.sources', label: 'your sources' },
      { id: 'sp.renderer', label: 'the page browser' },
    ],
    { centre: this.centre, autoSettle: true },
  );
  readonly mockJobs = new MockJobs(Number(new URLSearchParams(globalThis.location?.search ?? '').get('jobStepMs') ?? 350));
  readonly jobs = new MemoryJobsBackend((id) => this.mockJobs.cancel(id));
  /** Only the shell's Appearance section: Spotter's own settings are its page in the window. */
  readonly settings = new MemorySettingsBackend({ ...SETTINGS_OVERVIEW_FIXTURE, sections: SETTINGS_OVERVIEW_FIXTURE.sections.filter((s) => s.id === 'pd.appearance') }, {});

  constructor() {
    this.mockJobs.connect((s) => this.jobs.emit(s));
    // The mock's "broken" scenario is an engine that cannot reach anything: the connection service says so, as Rust would.
    if (new URLSearchParams(globalThis.location?.search ?? '').get('mock') === 'broken') this.breakSources();
  }

  /** The scenario "Break the connection" of the browser build (`?health=sources`): every watch unreachable. */
  breakSources(): void {
    this.health.fail('sp.sources', cause('Spotter cannot reach any of the places it watches.', 'Check the network connection, then check again.', 'dns error'));
  }
}

let about: AboutBackend | null = null;
let updates: UpdatesBackend | null = null;

/** The single source of the browser build's version: the stand-in's, read by the specs from the same place. */
export const BROWSER_VERSION = '1.0.0';

/** About over Tauri in the app; in the browser one in-memory engine per page. */
export function aboutBackend(): AboutBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriAboutBackend(ipc);
  about ??= new MemoryAboutBackend(
    {
      appName: 'Spotter',
      version: BROWSER_VERSION,
      os: 'browser',
      arch: 'web',
      commit: null,
      built: null,
      homepage: null,
      releasesPage: 'https://dev.decryptology.net/decryp7/spotter/releases',
      legal: [{ id: 'mit', title: 'MIT licence' }],
      updatesAvailable: true,
    },
    { mit: 'MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software…' },
    { version: BROWSER_VERSION, release: { version: BROWSER_VERSION, date: null, groups: [{ title: 'Added', items: ['Runs on the Paddock app shell.'] }] } },
  );
  return about;
}

/** The updater over Tauri in the app; in the browser a fake update server. */
export function updatesBackend(): UpdatesBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriUpdatesBackend(ipc);
  updates ??= new MemoryUpdatesBackend({
    currentVersion: BROWSER_VERSION,
    server: 'none',
    release: { version: '99.0.0', notes: 'A sample release of the fake update server.', pubDate: null, size: 41_000_000 },
    channels: [{ id: 'stable', label: 'Stable' }],
  });
  return updates;
}

/** Settings over Tauri in the app; in the browser the in-memory engine. */
export function settingsBackend(): SettingsBackend {
  const ipc = tauriIpc();
  return ipc ? new TauriSettingsBackend(ipc) : inject(BrowserEngines).settings;
}

/** Jobs over Tauri in the app; in the browser the in-memory engine. */
export function jobsBackend(): JobsBackend {
  const ipc = tauriIpc();
  return ipc ? new TauriJobsBackend(ipc) : inject(BrowserEngines).jobs;
}

/** Health and notifications over Tauri in the app; in the browser the in-memory engine. */
export function notifyBackends(): { health: () => ReturnType<typeof healthOf>; notifications: () => ReturnType<typeof centreOf> } {
  return { health: healthOf, notifications: centreOf };
}
function healthOf() {
  const ipc = tauriIpc();
  return ipc ? new TauriServiceHealthBackend(ipc) : inject(BrowserEngines).health;
}
function centreOf() {
  const ipc = tauriIpc();
  return ipc ? new TauriNotificationsBackend(ipc) : inject(BrowserEngines).centre;
}
