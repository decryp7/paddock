import { Injector, inject, provideAppInitializer } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { provideContributions, provideShellContext, provideShellVersionCheck } from '@decryp7/paddock-ui/shell';
import { LANGUAGE_CONFIG, LocalStorageLanguageStorage } from '@decryp7/paddock-ui/shell/i18n';
import { provideShellAbout } from '@decryp7/paddock-ui/shell/about';
import { tauriDialogIo } from '@decryp7/paddock-ui/shell/dialogs';
import { JobsStore, MemoryJobsBackend, TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { TauriNotificationsBackend, TauriServiceHealthBackend } from '@decryp7/paddock-ui/shell/notify';
import { provideShellPages } from '@decryp7/paddock-ui/shell/pages';
import { provideShellSettings, SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { MockStartupBackend, reportBootstrapFailure, TauriStartupBackend, windowReady } from '@decryp7/paddock-ui/shell/startup';
import { BACKEND, isTauri, type PitcrewBackend } from '../core/backend';
import { provideDeepLinks } from '../core/deep-link';
import { MockBackend, type MockScenario } from '../core/mock-backend';
import { provideNativeFeel } from '../core/native-feel';
import { TauriBackend } from '../core/tauri-backend';
import { provideIcons } from '../shared/icons';
import { aboutBackend, updatesBackend } from './about';
import { providePitcrewAppearance } from './appearance';
import { BrowserHealth, browserJobs } from './browser-engine';
import { tauriBridge, tauriIpc } from './ipc';
import { APP_NAME } from './names';
import { PitcrewIcon } from './pitcrew-icon';
import { PitcrewRoot } from './pitcrew-root';
import { ABOUT_CONTRIBUTIONS, PITCREW_CONTRIBUTIONS, PITCREW_ROUTES } from './pitcrew.contributions';
import { settingsBackend } from './settings';
import { followJobsStartedElsewhere } from './window-jobs';

const SCENARIOS: MockScenario[] = ['demo', 'empty', 'unavailable', 'notInstalled', 'mismatch'];

/**
 * The browser build's mock; `?mock=<scenario>&platform=<os>` picks what it shows (development and screenshots).
 * `?mock-down` takes the service down (its controls keep working), `?mock-startfailed` shows launchd refusing it after an update.
 */
function browserBackend(): PitcrewBackend {
  // Looked up when a job starts: the jobs store needs the shell's ports, which need this backend (no cycle at creation).
  const injector = inject(Injector);
  const params = new URLSearchParams(globalThis.location?.search ?? '');
  const scenario = params.get('mock') as MockScenario | null;
  const platform = params.get('platform');
  return new MockBackend({
    scenario: scenario && SCENARIOS.includes(scenario) ? scenario : 'demo',
    latencyMs: 120,
    simulate: params.get('simulate') !== '0',
    serverDown: params.has('mock-down'),
    startFailed: params.has('mock-startfailed'),
    platform: platform === 'macos' || platform === 'windows' || platform === 'linux' ? platform : 'browser',
    jobs: browserJobs,
    follow: (id) => injector.get(JobsStore).follow(id),
  });
}

/** Pitcrew's main window: the pages frame (ADR 0012) from Pitcrew's contributions. */
export function bootMainWindow(): Promise<unknown> {
  const ipc = tauriIpc();
  const bridge = tauriBridge();
  return bootstrapApplication(PitcrewRoot, {
    providers: [
      providePitcrewAppearance(),
      { provide: LANGUAGE_CONFIG, useValue: { storage: new LocalStorageLanguageStorage(), systemLanguages: () => navigator.languages, shipped: [], reload: () => location.reload() } },
      provideIcons(),
      provideNativeFeel(),
      // Inside the app the real Tauri backend; in the browser build (development, UI tests) the mock.
      {
        provide: BACKEND,
        useFactory: () => {
          if (!isTauri()) return browserBackend();
          // Jobs the pages start are waited for through the window's jobs store, so the shell does not announce their end too.
          const injector = inject(Injector);
          return new TauriBackend(undefined, undefined, (id) => injector.get(JobsStore).follow(id));
        },
      },
      // A registration the register window starts is not announced a second time here: its wizard says how it ended.
      provideAppInitializer(() => followJobsStartedElsewhere()),
      provideAppInitializer(() => {
        const backend = inject(BACKEND);
        if (backend instanceof TauriBackend) void backend.init();
      }),
      provideContributions(PITCREW_CONTRIBUTIONS),
      ...(ipc ? [provideShellVersionCheck(ipc.invoke)] : []),
      provideContributions(ABOUT_CONTRIBUTIONS),
      provideShellAbout({ about: aboutBackend, updates: updatesBackend, main: true, ready: windowReady }),
      provideContributions(SETTINGS_CONTRIBUTIONS),
      provideShellSettings({ backend: settingsBackend, appearance: { gate: windowReady } }),
      // The app's part of the context is a lazy chunk, loaded when the window is idle.
      provideShellContext(() => import('./pitcrew-context').then((m) => m.pitcrewContext)),
      provideDeepLinks(),
      // The whole window from the contributions: pages, status bar, menus and keys, notifications and service health, jobs
      // ("Still working" after 10 s without events, never a timeout), the start-up contract, and dialogs as OS windows.
      provideShellPages({
        appName: APP_NAME,
        icon: PitcrewIcon,
        routes: PITCREW_ROUTES,
        notify: ipc
          ? { health: () => new TauriServiceHealthBackend(ipc), notifications: () => new TauriNotificationsBackend(ipc) }
          : { health: () => inject(BrowserHealth).health, notifications: () => inject(BrowserHealth).centre },
        jobs: { backend: () => (ipc ? new TauriJobsBackend(ipc) : (browserJobs as MemoryJobsBackend)) },
        startup: { backend: () => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend()) },
        ...(ipc ? { nativeMenu: tauriNativeMenuPort(ipc) } : {}),
        ...(bridge ? { dialogs: { io: () => tauriDialogIo(bridge) } } : {}),
      }),
    ],
  }).catch((e: unknown) => {
    console.error(e);
    reportBootstrapFailure(e);
  });
}
