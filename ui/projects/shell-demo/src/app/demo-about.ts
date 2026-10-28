import { inject } from '@angular/core';
import { MemoryAboutBackend, MemoryUpdatesBackend, TauriAboutBackend, TauriUpdatesBackend, type AboutBackend, type FakeServer, type UpdatesBackend } from '@decryp7/paddock-ui/shell/about';
import shellPackage from '../../../paddock-ui/package.json';
import { APP_NAME } from './demo-name';
import { tauriIpc } from './demo-ipc';
import { DemoJobsEngine } from './demo-jobs';

/** The demo's version: the shell's own (its single source, rule 6; the e2e reads the same file). */
export const DEMO_VERSION: string = shellPackage.version;

const SERVERS: readonly FakeServer[] = ['none', 'release', 'bad-signature', 'offline', 'unavailable'];

/** Which fake update server the browser build plays (`?updates=bad-signature`); a newer release by default. */
export function fakeServer(search: string = globalThis.location?.search ?? ''): FakeServer {
  const wanted = new URLSearchParams(search).get('updates');
  return SERVERS.find((s) => s === wanted) ?? 'release';
}

/** `?hold`, read when the module loads: the router replaces the URL (and its query) once the window is up. */
const HOLD = new URLSearchParams(globalThis.location?.search ?? '').has('hold');

let about: MemoryAboutBackend | null = null;
let updates: UpdatesBackend | null = null;

/**
 * About over Tauri in the desktop build; in the browser, one in-memory engine per page (the main window and its in-page
 * dialogs share it, as windows share the real engine).
 */
export function demoAboutBackend(): AboutBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriAboutBackend(ipc);
  about ??= new MemoryAboutBackend(
    {
      appName: APP_NAME,
      version: DEMO_VERSION,
      os: 'browser',
      arch: 'web',
      commit: null,
      built: null,
      homepage: null,
      releasesPage: 'https://dev.decryptology.net/decryp7/paddock/releases',
      legal: [
        { id: 'notices', title: 'Third-party notices' },
        { id: 'mit', title: 'MIT licence' },
      ],
      updatesAvailable: fakeServer() !== 'unavailable',
    },
    {
      notices: 'Third-party notices (sample)\n\nAngular — MIT\nNgRx — MIT\nApache ECharts — Apache-2.0\n',
      mit: 'MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software…',
    },
    {
      version: DEMO_VERSION,
      release: {
        version: DEMO_VERSION,
        date: null,
        groups: [
          { title: 'Added', items: ['About, Updates and What’s New as shell dialogs (sample notes of the demo).'] },
          { title: 'Fixed', items: ['What’s New shows the running version’s notes only.'] },
        ],
      },
    },
  );
  // The browser build has no engine to ask: the releases link opens in a new tab.
  about.opener = (url) => void window.open(url, '_blank', 'noopener,noreferrer');
  return about;
}

/** What `?hold` puts on `window.sdUpdatesDownload`: step the held download by hand (the e2e and the screenshots). */
export interface HeldDownload {
  /** Sends the download job's phase to the window's Jobs (a total of `null` is a count that has no end yet). */
  progress(done: number, total: number | null): void;
  /** Lets the download finish (the signature check and the result). */
  finish(): void;
}

/**
 * The updater over Tauri in the desktop build; in the browser, the fake update server of `?updates=`. With `?hold` (browser
 * only) the download stays running until `window.sdUpdatesDownload.finish()`, and `progress(done, total)` plays the engine's
 * job events, so the progress bar can be seen and tested.
 */
export function demoUpdatesBackend(): UpdatesBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriUpdatesBackend(ipc);
  if (updates) return updates;
  const hold = HOLD;
  const jobs = hold ? inject(DemoJobsEngine).backend : null;
  const held: (() => void)[] = [];
  let downloadJob = -1;
  let downloading = false;
  let seq = 0;
  const memory = new MemoryUpdatesBackend({
    currentVersion: DEMO_VERSION,
    server: fakeServer(),
    release: { version: '99.0.0', notes: 'A sample release of the fake update server.', pubDate: null, size: 41_000_000 },
    channels: [
      { id: 'stable', label: 'Stable' },
      { id: 'beta', label: 'Beta' },
    ],
    schedule: (step) => (hold && downloading ? held.push(step) : queueMicrotask(step)),
  });
  if (jobs) {
    void memory.listen((status) => {
      downloading = status.phase.kind === 'downloading';
      if (status.phase.kind === 'downloading') downloadJob = status.phase.job ?? -1;
    });
    const api: HeldDownload = {
      progress: (done, total) =>
        jobs.emit({
          id: downloadJob,
          kind: 'update-download',
          title: 'Downloading the update',
          subject: '99.0.0',
          seq: ++seq,
          elapsedMs: 0,
          etaMs: null,
          cancellable: true,
          notCancellableReason: null,
          state: 'running',
          phase: { id: 'download', label: 'Downloading', unit: 'bytes', total, done },
        }),
      finish: () => held.splice(0).forEach((step) => step()),
    };
    (globalThis as { sdUpdatesDownload?: HeldDownload }).sdUpdatesDownload = api;
  }
  updates = memory;
  return updates;
}
