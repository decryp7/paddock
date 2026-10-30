import { MemoryAboutBackend, MemoryUpdatesBackend, TauriAboutBackend, TauriUpdatesBackend, type AboutBackend, type FakeServer, type UpdatesBackend } from '@decryp7/paddock-ui/shell/about';
import appPackage from '../../../package.json';
import { tauriIpc } from './ipc';
import { APP_NAME } from './names';

/** The version the browser build shows: package.json's (the desktop app answers its Cargo.toml version; rule 6). */
export const BROWSER_VERSION: string = appPackage.version;

const SERVERS: readonly FakeServer[] = ['none', 'release', 'bad-signature', 'offline', 'unavailable'];

/** Which fake update server the browser build plays (`?updates=bad-signature`); no update by default. */
export function fakeServer(search: string = globalThis.location?.search ?? ''): FakeServer {
  const wanted = new URLSearchParams(search).get('updates');
  return SERVERS.find((s) => s === wanted) ?? 'none';
}

let about: AboutBackend | null = null;
let updates: UpdatesBackend | null = null;

/** About over Tauri in the desktop build; in the browser one in-memory engine per page. */
export function aboutBackend(): AboutBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriAboutBackend(ipc);
  about ??= new MemoryAboutBackend(
    {
      appName: APP_NAME,
      version: BROWSER_VERSION,
      os: 'Windows 11',
      arch: 'x86_64',
      commit: null,
      built: null,
      homepage: 'https://dev.decryptology.net/decryp7/paddock',
      releasesPage: 'https://dev.decryptology.net/decryp7/paddock/releases?q=pitcrew-v',
      legal: [{ id: 'licence-mit', title: 'MIT licence' }],
      updatesAvailable: fakeServer() !== 'unavailable',
    },
    { 'licence-mit': 'MIT License (the browser build shows a stand-in; the app carries the real licences).' },
    {
      version: BROWSER_VERSION,
      // The browser build's sample notes (the app shows its CHANGELOG.md).
      release: {
        version: BROWSER_VERSION,
        date: '2026-10-22',
        groups: [
          { title: 'Changed', items: ['Pitcrew runs on the Paddock app shell: one frame, notifications and service health with Restart, and jobs with progress and Cancel.', 'Cleaning a disk, registering a runner and trimming the Actions cache run as jobs that can be cancelled.'] },
          { title: 'Fixed', items: ['The log viewer fills the page.'] },
        ],
      },
    },
  );
  return about;
}

/** The updater over Tauri in the desktop build; in the browser the fake update server of `?updates=`. */
export function updatesBackend(): UpdatesBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriUpdatesBackend(ipc);
  updates ??= new MemoryUpdatesBackend({
    currentVersion: BROWSER_VERSION,
    server: fakeServer(),
    release: { version: '99.0.0', notes: 'A sample release of the fake update server.', pubDate: null, size: 41_000_000 },
    channels: [{ id: 'stable', label: 'Stable' }],
  });
  return updates;
}
