import { inject, Injectable } from '@angular/core';
import {
  MemoryAboutBackend,
  MemoryUpdatesBackend,
  TauriAboutBackend,
  TauriUpdatesBackend,
  type AboutBackend,
  type UpdatesBackend,
} from '@decryp7/paddock-ui/shell/about';
import { MemoryJobsBackend, TauriJobsBackend, type JobsBackend, type JobsIpc } from '@decryp7/paddock-ui/shell/jobs';
import { MemoryNotificationCentre, MemoryServiceHealth, TauriNotificationsBackend, TauriServiceHealthBackend } from '@decryp7/paddock-ui/shell/notify';
import { MemorySettingsBackend, SETTINGS_OVERVIEW_FIXTURE, TauriSettingsBackend, type SettingsBackend } from '@decryp7/paddock-ui/shell/settings';
import { tauriIpc, type DynoIpc } from './ipc';

/** The shell's service for the Dyno agent (the Rust health table registers the same id). */
export const AGENT_SERVICE = 'dy.agent';

/**
 * The browser build's stand-ins for the shell's Rust parts (the browser has no Rust): service health with Dyno's one
 * service, the notification centre, jobs, settings and About/updates. The shell's UI code is the same as in the app; only
 * the backends are in memory. One instance per page, as the real engine is one for all windows.
 */
@Injectable({ providedIn: 'root' })
export class BrowserEngines {
  readonly centre = new MemoryNotificationCentre();
  readonly health = new MemoryServiceHealth([{ id: AGENT_SERVICE, label: 'the Dyno agent' }], { centre: this.centre, autoSettle: true });
  /** What the browser build's mock does when the Jobs view cancels a job (the mock registers itself here). */
  cancelJob: (id: number) => void = () => undefined;
  readonly jobs = new MemoryJobsBackend((id) => this.cancelJob(id));
  /** Only the shell's Appearance section: Dyno's own settings are its page in the window. */
  readonly settings = new MemorySettingsBackend({ ...SETTINGS_OVERVIEW_FIXTURE, sections: SETTINGS_OVERVIEW_FIXTURE.sections.filter((s) => s.id === 'pd.appearance') }, {});
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
      appName: 'Dyno',
      version: BROWSER_VERSION,
      os: 'browser',
      arch: 'web',
      commit: null,
      built: null,
      homepage: null,
      releasesPage: 'https://dev.decryptology.net/decryp7/paddock/releases?q=dyno-v',
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
    host: 'dev.decryptology.net',
    releasesPage: 'https://dev.decryptology.net/decryp7/paddock/releases?q=dyno-v',
    release: { version: '1.1.0', notes: 'Disk map, faster history charts and fixes.', pubDate: null, size: 41_000_000 },
    channels: [{ id: 'stable', label: 'Stable' }],
  });
  return updates;
}

/** Settings over Tauri in the app; in the browser the in-memory engine. */
export function settingsBackend(): SettingsBackend {
  const ipc = tauriIpc();
  return ipc ? new TauriSettingsBackend(ipc) : inject(BrowserEngines).settings;
}

/**
 * The jobs port over Dyno's agent. The jobs run in the agent, not in this process, so the shell's `jobs_snapshot` and
 * `job_cancel` are answered by forwarding to the agent through the one `dyno` command (`jobs`, `cancelJob`); the agent's job
 * statuses reach the window as `paddock://jobs` events, relayed by the shell from the link's push.
 */
export function dynoJobsIpc(ipc: Pick<DynoIpc, 'invoke' | 'listen'>): JobsIpc {
  return {
    invoke: <T>(command: string, args?: Record<string, unknown>): Promise<T> =>
      command === 'jobs_snapshot'
        ? ipc.invoke<T>('dyno', { request: 'jobs' })
        : ipc.invoke<T>('dyno', { request: { cancelJob: { id: args?.['id'] } } }),
    listen: (event, handler) => ipc.listen(event, handler),
  };
}

/** Jobs over the agent in the app; in the browser the in-memory engine the mock pushes into. */
export function jobsBackend(): JobsBackend {
  const ipc = tauriIpc();
  return ipc ? new TauriJobsBackend(dynoJobsIpc(ipc)) : inject(BrowserEngines).jobs;
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
