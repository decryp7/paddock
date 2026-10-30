import { inject, Injectable } from '@angular/core';
import {
  MemoryAboutBackend,
  MemoryUpdatesBackend,
  TauriAboutBackend,
  TauriUpdatesBackend,
  type AboutBackend,
  type UpdatesBackend,
} from '@decryp7/paddock-ui/shell/about';
import { MemoryNotificationCentre, MemoryServiceHealth, TauriNotificationsBackend, TauriServiceHealthBackend } from '@decryp7/paddock-ui/shell/notify';
import { MemorySettingsBackend, SETTINGS_OVERVIEW_FIXTURE, TauriSettingsBackend, type SettingsBackend } from '@decryp7/paddock-ui/shell/settings';
import { tauriIpc } from './ipc';

/**
 * The browser build's stand-ins for the shell's Rust parts (the browser has no Rust): the notification centre, service
 * health (Pitwall registers no shell service: its server's state is its own status item and notices), settings and
 * About/updates. The shell's UI code is the same as in the app; only the backends are in memory. One instance per page,
 * as the real engine is one for all windows.
 */
@Injectable({ providedIn: 'root' })
export class BrowserEngines {
  readonly centre = new MemoryNotificationCentre();
  readonly health = new MemoryServiceHealth([], { centre: this.centre, autoSettle: true });
  /** Only the shell's Appearance section: Pitwall's own settings are its page in the window. */
  readonly settings = new MemorySettingsBackend({ ...SETTINGS_OVERVIEW_FIXTURE, sections: SETTINGS_OVERVIEW_FIXTURE.sections.filter((s) => s.id === 'pd.appearance') }, {});
}

let about: AboutBackend | null = null;
let updates: UpdatesBackend | null = null;

/** The single source of the browser build's version: the stand-in's, read by the specs from the same place. */
export const BROWSER_VERSION = '1.1.0';

/** Where Pitwall's releases are (Rust's `RELEASES_PAGE`). */
export const RELEASES_PAGE = 'https://dev.decryptology.net/decryp7/paddock/releases?q=pitwall-v';

/** About over Tauri in the app; in the browser one in-memory engine per page. */
export function aboutBackend(): AboutBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriAboutBackend(ipc);
  about ??= new MemoryAboutBackend(
    {
      appName: 'Pitwall',
      version: BROWSER_VERSION,
      os: 'browser',
      arch: 'web',
      commit: null,
      built: null,
      homepage: 'https://dev.decryptology.net/decryp7/paddock',
      releasesPage: RELEASES_PAGE,
      legal: [
        { id: 'notices', title: 'Third-party notices' },
        { id: 'licence-mit', title: 'MIT licence' },
        { id: 'licence-apache', title: 'Apache licence 2.0' },
      ],
      updatesAvailable: true,
    },
    {
      notices: '# Third-party notices\n\nPitwall ships open-source software. The desktop app lists every Rust crate and npm package with its licence text here.',
      'licence-mit': 'MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software…',
      'licence-apache': 'Apache License\nVersion 2.0, January 2004\n\nhttp://www.apache.org/licenses/',
    },
    { version: BROWSER_VERSION, release: { version: BROWSER_VERSION, date: null, groups: [{ title: 'Changed', items: ['Pitwall runs on the Paddock app shell: Settings, About and Updates are their own windows.'] }] } },
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
    host: 'dev.decryptology.net',
    releasesPage: RELEASES_PAGE,
    release: { version: '1.1.1', notes: 'Faster model loading and fixes.', pubDate: null, size: 38_000_000 },
    channels: [{ id: 'stable', label: 'Stable' }],
  });
  return updates;
}

/** Settings over Tauri in the app; in the browser the in-memory engine. */
export function settingsBackend(): SettingsBackend {
  const ipc = tauriIpc();
  return ipc ? new TauriSettingsBackend(ipc) : inject(BrowserEngines).settings;
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
