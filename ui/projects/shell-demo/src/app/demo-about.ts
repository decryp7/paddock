import { MemoryAboutBackend, MemoryUpdatesBackend, TauriAboutBackend, TauriUpdatesBackend, type AboutBackend, type FakeServer, type UpdatesBackend } from '@decryp7/paddock-ui/shell/about';
import shellPackage from '../../../paddock-ui/package.json';
import { APP_NAME } from './demo-name';
import { tauriIpc } from './demo-ipc';

/** The demo's version: the shell's own (its single source, rule 6; the e2e reads the same file). */
export const DEMO_VERSION: string = shellPackage.version;

const SERVERS: readonly FakeServer[] = ['none', 'release', 'bad-signature', 'offline', 'unavailable'];

/** Which fake update server the browser build plays (`?updates=bad-signature`); a newer release by default. */
export function fakeServer(search: string = globalThis.location?.search ?? ''): FakeServer {
  const wanted = new URLSearchParams(search).get('updates');
  return SERVERS.find((s) => s === wanted) ?? 'release';
}

let about: AboutBackend | null = null;
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
  return about;
}

/** The updater over Tauri in the desktop build; in the browser, the fake update server of `?updates=`. */
export function demoUpdatesBackend(): UpdatesBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriUpdatesBackend(ipc);
  updates ??= new MemoryUpdatesBackend({
    currentVersion: DEMO_VERSION,
    server: fakeServer(),
    release: { version: '99.0.0', notes: 'A sample release of the fake update server.', pubDate: null, size: 41_000_000 },
    channels: [
      { id: 'stable', label: 'Stable' },
      { id: 'beta', label: 'Beta' },
    ],
  });
  return updates;
}
