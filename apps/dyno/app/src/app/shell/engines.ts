import { inject, Injectable } from '@angular/core';
import {
  MemoryAboutBackend,
  MemoryUpdatesBackend,
  TauriAboutBackend,
  TauriUpdatesBackend,
  type AboutBackend,
  type UpdatesBackend,
} from '@decryp7/paddock-ui/shell/about';
import { mockOptionsFromUrl } from '../core/mock-backend';
import { MemoryDiagnosticsBackend, TauriDiagnosticsBackend, type DiagnosticsBackend } from '@decryp7/paddock-ui/shell/diagnostics';
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

  constructor() {
    // Mock only: `?seed=network-alarms` lists what Dyno's network rules post (the engine's own text), without a toast, for the layout checks and screenshots.
    if (typeof window !== 'undefined' && new URLSearchParams(window.location.search).get('seed') === 'network-alarms') seedNetworkAlarms(this.centre);
  }
}

/** The text of `network_alerts.rs`: the title names the program, the body adds only what the title lacks. Invented hosts (TEST-NET addresses). */
export function seedNetworkAlarms(centre: MemoryNotificationCentre): void {
  const post = (cause: string, severity: 'info' | 'warning', title: string, body: string): void => void centre.post({ cause, source: 'alarm', severity, title, body, quiet: true });
  post('network:newRemoteHost:browser:203.0.113.17', 'info', 'browser.exe connected to a new host', 'Address 203.0.113.17:443');
  post('network:newRemoteHost:mailer:198.51.100.23', 'info', 'mailer.exe connected to a new host', 'Address 198.51.100.23:993');
  post('network:newExposedListener:node:tcp:3000', 'warning', 'node started listening on port 3000', 'TCP, on every network interface (reachable from the network).');
}

let about: AboutBackend | null = null;
let updates: UpdatesBackend | null = null;

/** The single source of the browser build's version: the stand-in's, read by the specs from the same place. */
export const BROWSER_VERSION = '1.0.0';

/** About over Tauri in the app; in the browser one in-memory engine per page. */
export function aboutBackend(): AboutBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriAboutBackend(ipc);
  const mockOs = mockOptionsFromUrl(typeof window === 'undefined' ? '' : window.location.search).os ?? 'macos';
  about ??= new MemoryAboutBackend(
    {
      appName: 'Dyno',
      version: BROWSER_VERSION,
      // The mock machine's own system (`?mock=windows|macos|linux`), as the engine reports it, not the browser it runs in.
      os: mockOs,
      arch: mockOs === 'macos' ? 'aarch64' : 'x86_64',
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

/**
 * Browser build only, for the e2e: a page that sets `__pdHoldUpdatesStatus` before it loads gets its first status only when
 * it calls `__pdReleaseUpdatesStatus()`, so a window sees the status arrive after its first measure (as on a slow IPC).
 */
function heldStatus(): Promise<void> | undefined {
  const g = globalThis as { __pdHoldUpdatesStatus?: boolean; __pdReleaseUpdatesStatus?: () => void };
  if (!g.__pdHoldUpdatesStatus) return undefined;
  return new Promise<void>((release) => (g.__pdReleaseUpdatesStatus = release));
}

/** The updater over Tauri in the app; in the browser a fake update server. */
export function updatesBackend(): UpdatesBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriUpdatesBackend(ipc);
  updates ??= new MemoryUpdatesBackend({
    holdStatus: heldStatus(),
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

let diagnostics: DiagnosticsBackend | null = null;

/**
 * Report a problem over Tauri in the app; in the browser one in-memory engine per page, holding the files a real report holds for
 * Dyno: the system facts, the shell's settings, and the dated logs of BOTH processes (the app's `dyno.<day>.log` and the agent's
 * `dyno-agent.<day>.log`, which the real engine reads from disk, so they are there with the agent down).
 */
export function diagnosticsBackend(): DiagnosticsBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriDiagnosticsBackend(ipc);
  diagnostics ??= new MemoryDiagnosticsBackend({
    save: 'save',
    files: [
      { name: 'system-info.json', kind: 'system-info', bytes: 412, summary: 'Dyno, your operating system and processor type, no hardware serial numbers' },
      { name: 'settings.json', kind: 'settings', bytes: 380, summary: 'Your settings; passwords, tokens and keys are replaced by ***' },
      { name: 'logs/dyno.2026-10-31.log', kind: 'log', bytes: 182_400, summary: '1,204 log lines, the recent part of dyno.2026-10-31.log; file contents are never logged' },
      { name: 'logs/dyno-agent.2026-10-31.log', kind: 'log', bytes: 36_100, summary: '240 log lines, all of dyno-agent.2026-10-31.log; file contents are never logged' },
    ],
  });
  return diagnostics;
}
