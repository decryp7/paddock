import type {
  AboutInfo,
  ChannelInfo,
  InstallInfo,
  Preferences,
  UpdateErrorDto,
  UpdateInfo,
  UpdatePhase,
  UpdateStatus,
  WhatsNew,
  WhatsNewDecision,
} from '../generated/about';
import { UPDATES_EVENT } from '../generated/about';
import type { AboutCommand } from '../generated/about-commands';
import { parseAboutInfo, parseUpdateStatus, parseWhatsNew, parseWhatsNewDecision } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The app's IPC as these backends need it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke`
 * and an adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface AboutIpc {
  invoke<T>(command: AboutCommand | 'files_open_url', args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/** About's port (`paddock_updater::tauri`): the running version, legal texts and release notes, all from the engine. */
export abstract class AboutBackend {
  /** The running version, build facts and the legal documents' titles (`about_info`). */
  abstract info(): Promise<AboutInfo>;
  /** One legal document's text, fetched only when asked for (`about_legal`). */
  abstract legal(id: string): Promise<string>;
  /** The running version's release notes; `release` is null when there are none (`about_whats_new`). */
  abstract whatsNew(): Promise<WhatsNew>;
  /** Once per start, from the main window: whether to open What's New (`about_whats_new_start`). */
  abstract whatsNewOnStart(): Promise<WhatsNewDecision>;
  /** Opens a web address (the releases page) in the system browser (`files_open_url`, which refuses other schemes). */
  abstract openUrl(url: string): Promise<void>;
}

/**
 * The updater's port. Every method returns at once with the new status; the work (check, download, signature check)
 * runs as jobs in the engine and its results arrive by `listen`. Nothing here has a timeout (rule 12).
 */
export abstract class UpdatesBackend {
  abstract status(): Promise<UpdateStatus>;
  abstract check(): Promise<UpdateStatus>;
  abstract download(): Promise<UpdateStatus>;
  /** Stops the check or download in flight, or drops the verified download (the update stays available). */
  abstract cancel(): Promise<UpdateStatus>;
  abstract installOnQuit(enabled: boolean): Promise<UpdateStatus>;
  /** Quits, installs and starts the new version. */
  abstract restart(): Promise<UpdateStatus>;
  abstract setPreferences(preferences: Preferences): Promise<UpdateStatus>;
  /** Every status the engine sends (`paddock://updates`) until the returned function is called. */
  abstract listen(handler: (status: UpdateStatus) => void): Promise<Unlisten>;
}

function parsed<T>(value: T | null, what: string): T {
  if (value === null) throw new Error(`The engine sent no valid ${what}.`);
  return value;
}

/** About over Tauri. Payloads are parsed at this edge. */
export class TauriAboutBackend extends AboutBackend {
  constructor(private readonly ipc: AboutIpc) {
    super();
  }

  async info(): Promise<AboutInfo> {
    return parsed(parseAboutInfo(await this.ipc.invoke<unknown>('about_info')), 'About information');
  }

  async legal(id: string): Promise<string> {
    const text = await this.ipc.invoke<unknown>('about_legal', { id });
    return parsed(typeof text === 'string' ? text : null, 'legal text');
  }

  async whatsNew(): Promise<WhatsNew> {
    return parsed(parseWhatsNew(await this.ipc.invoke<unknown>('about_whats_new')), 'release notes');
  }

  async whatsNewOnStart(): Promise<WhatsNewDecision> {
    return parsed(parseWhatsNewDecision(await this.ipc.invoke<unknown>('about_whats_new_start')), 'start-up decision');
  }

  async openUrl(url: string): Promise<void> {
    await this.ipc.invoke<unknown>('files_open_url', { url });
  }
}

/** The updater over Tauri. A status that does not parse is refused (commands) or dropped (events). */
export class TauriUpdatesBackend extends UpdatesBackend {
  constructor(private readonly ipc: AboutIpc) {
    super();
  }

  private async call(command: AboutCommand, args?: Record<string, unknown>): Promise<UpdateStatus> {
    return parsed(parseUpdateStatus(await this.ipc.invoke<unknown>(command, args)), 'update status');
  }

  status(): Promise<UpdateStatus> {
    return this.call('updates_status');
  }
  check(): Promise<UpdateStatus> {
    return this.call('updates_check');
  }
  download(): Promise<UpdateStatus> {
    return this.call('updates_download');
  }
  cancel(): Promise<UpdateStatus> {
    return this.call('updates_cancel');
  }
  installOnQuit(enabled: boolean): Promise<UpdateStatus> {
    return this.call('updates_install_on_quit', { enabled });
  }
  restart(): Promise<UpdateStatus> {
    return this.call('updates_restart');
  }
  setPreferences(preferences: Preferences): Promise<UpdateStatus> {
    return this.call('updates_set_preferences', { preferences });
  }
  listen(handler: (status: UpdateStatus) => void): Promise<Unlisten> {
    return this.ipc.listen(UPDATES_EVENT, (payload) => {
      const status = parseUpdateStatus(payload);
      if (status) handler(status);
    });
  }
}

/** About held in memory (browser build, demos, tests). */
export class MemoryAboutBackend extends AboutBackend {
  constructor(
    private readonly about: AboutInfo,
    private readonly texts: Readonly<Record<string, string>> = {},
    private readonly notes: WhatsNew = { version: about.version, release: null },
    private decision: WhatsNewDecision = 'nothing',
  ) {
    super();
  }

  /** How often a legal text was fetched (it is fetched only on request). */
  legalReads = 0;

  info(): Promise<AboutInfo> {
    return Promise.resolve(this.about);
  }

  legal(id: string): Promise<string> {
    this.legalReads++;
    const text = this.texts[id];
    return text === undefined ? Promise.reject(new Error(`there is no legal document '${id}'`)) : Promise.resolve(text);
  }

  whatsNew(): Promise<WhatsNew> {
    return Promise.resolve(this.notes);
  }

  whatsNewOnStart(): Promise<WhatsNewDecision> {
    const d = this.decision;
    this.decision = 'nothing';
    return Promise.resolve(d);
  }

  /** The web addresses asked to open, in order. */
  readonly openedUrls: string[] = [];
  /** Opens them, when set (a browser build passes `window.open`); tests leave it unset and read `openedUrls`. */
  opener: ((url: string) => void) | null = null;

  openUrl(url: string): Promise<void> {
    this.openedUrls.push(url);
    this.opener?.(url);
    return Promise.resolve();
  }
}

/** What the fake update server of [`MemoryUpdatesBackend`] has. */
export type FakeServer =
  /** Nothing newer than the running version. */
  | 'none'
  /** A newer release whose package is signed with the app's key. */
  | 'release'
  /** A newer release whose package is not signed with the app's key: the download is refused. */
  | 'bad-signature'
  /** No network. */
  | 'offline'
  /** A build without an update key: nothing is ever checked. */
  | 'unavailable';

export interface MemoryUpdatesOptions {
  readonly currentVersion: string;
  readonly server: FakeServer;
  /** The newer release (`release`, `bad-signature`). */
  readonly release?: Omit<UpdateInfo, 'currentVersion'>;
  readonly channels?: readonly ChannelInfo[];
  /** The update server's host and releases page the Updates window shows (default: a placeholder host). */
  readonly host?: string;
  readonly releasesPage?: string;
  /** How the app is installed (default: per user, with Install when the app quits). */
  readonly install?: InstallInfo;
  /** Runs each simulated step of the engine's work (default: the next microtask). */
  readonly schedule?: (step: () => void) => void;
  /** `status()` answers only once this settles: a spec or an e2e that needs the status to arrive late, after a window's first measure. */
  readonly holdStatus?: Promise<void>;
}

const ERRORS: Readonly<Record<'notAvailable' | 'offline' | 'signatureInvalid' | 'busy', UpdateErrorDto>> = {
  notAvailable: {
    kind: 'notAvailable',
    message: 'Updates are not available in this build.',
    nextStep: 'This build was made without an update signing key, so it cannot verify updates. Download new versions from the releases page.',
    detail: null,
  },
  offline: { kind: 'offline', message: 'The network could not be reached.', nextStep: 'Check your internet connection or proxy, then try again.', detail: null },
  signatureInvalid: {
    kind: 'signatureInvalid',
    message: "The update's signature is not valid, so the update was refused.",
    nextStep: 'The download was deleted. Do not install it by hand; check for updates again, and report this to the maintainers if it repeats.',
    detail: "the update was signed with another key than this app's",
  },
  busy: { kind: 'busy', message: 'Wait for the current step to finish.', nextStep: 'Wait for the current step to finish.', detail: null },
};

/**
 * The updater held in memory, playing `paddock-updater` against a fake update server (no network): the same phases,
 * errors and refusals as the engine, so the shell's UI is the same as in the app. The engine is the reference; this
 * only mimics its contract.
 */
export class MemoryUpdatesBackend extends UpdatesBackend {
  private readonly listeners = new Set<(s: UpdateStatus) => void>();
  private current: UpdateStatus;
  private job = 0;
  private generation = 0;
  private readonly schedule: (step: () => void) => void;

  constructor(private readonly options: MemoryUpdatesOptions) {
    super();
    this.schedule = options.schedule ?? ((step) => queueMicrotask(step));
    const channels = options.channels ?? [{ id: 'stable', label: 'Stable' }];
    const unavailable = options.server === 'unavailable';
    this.current = {
      currentVersion: options.currentVersion,
      phase: unavailable ? { kind: 'unavailable' } : { kind: 'idle' },
      lastChecked: null,
      autoCheck: false,
      channel: channels[0]?.id ?? '',
      channels: [...channels],
      endpointHost: unavailable ? null : (options.host ?? 'updates.example'),
      releasesPage: options.releasesPage ?? 'https://updates.example/releases',
      install: options.install ?? { scope: 'perUser', installOnQuitAvailable: true, administratorNotice: null },
    };
  }

  private info(): UpdateInfo {
    const r = this.options.release ?? { version: '99.0.0', notes: '', pubDate: null, size: null };
    return { ...r, currentVersion: this.options.currentVersion };
  }

  private set(phase: UpdatePhase, extra: Partial<UpdateStatus> = {}): UpdateStatus {
    this.current = { ...this.current, ...extra, phase };
    for (const l of [...this.listeners]) l(this.current);
    return this.current;
  }

  private refuse(error: UpdateErrorDto): Promise<UpdateStatus> {
    return Promise.reject(error);
  }

  private later(step: () => void): void {
    const generation = this.generation;
    this.schedule(() => {
      if (generation === this.generation) step();
    });
  }

  status(): Promise<UpdateStatus> {
    if (this.options.holdStatus) return this.options.holdStatus.then(() => this.current);
    return Promise.resolve(this.current);
  }

  check(): Promise<UpdateStatus> {
    const kind = this.current.phase.kind;
    if (kind === 'unavailable') return this.refuse(ERRORS.notAvailable);
    if (!['idle', 'upToDate', 'available', 'failed'].includes(kind)) return this.refuse(ERRORS.busy);
    this.generation++;
    const status = this.set({ kind: 'checking', job: ++this.job });
    this.later(() => {
      const lastChecked = Math.floor(Date.now() / 1000);
      if (this.options.server === 'offline') this.set({ kind: 'failed', error: ERRORS.offline, info: null }, { lastChecked });
      else if (this.options.server === 'none') this.set({ kind: 'upToDate' }, { lastChecked });
      else this.set({ kind: 'available', info: this.info() }, { lastChecked });
    });
    return Promise.resolve(status);
  }

  download(): Promise<UpdateStatus> {
    const phase = this.current.phase;
    const retry = phase.kind === 'failed' && phase.info !== null && phase.error.kind !== 'signatureInvalid';
    if (phase.kind !== 'available' && !retry) return this.refuse({ ...ERRORS.busy, message: 'There is no update to download.' });
    this.generation++;
    const info = this.info();
    const status = this.set({ kind: 'downloading', info, job: ++this.job });
    this.later(() => {
      if (this.options.server === 'bad-signature') this.set({ kind: 'failed', error: ERRORS.signatureInvalid, info });
      else this.set({ kind: 'ready', info, installOnQuit: false });
    });
    return Promise.resolve(status);
  }

  cancel(): Promise<UpdateStatus> {
    const phase = this.current.phase;
    this.generation++;
    if (phase.kind === 'checking') return Promise.resolve(this.set({ kind: 'idle' }));
    if (phase.kind === 'downloading' || phase.kind === 'ready') return Promise.resolve(this.set({ kind: 'available', info: phase.info }));
    return this.refuse(phase.kind === 'unavailable' ? ERRORS.notAvailable : { ...ERRORS.busy, message: 'There is no download to cancel.' });
  }

  installOnQuit(enabled: boolean): Promise<UpdateStatus> {
    const phase = this.current.phase;
    if (phase.kind !== 'ready') return this.refuse({ ...ERRORS.busy, message: 'No verified update is ready to install.' });
    return Promise.resolve(this.set({ ...phase, installOnQuit: enabled }));
  }

  restart(): Promise<UpdateStatus> {
    const phase = this.current.phase;
    if (phase.kind !== 'ready') return this.refuse({ ...ERRORS.busy, message: 'No verified update is ready to install.' });
    // The app would quit here; the browser build stays on "installing".
    return Promise.resolve(this.set({ kind: 'installing', info: phase.info }));
  }

  setPreferences(preferences: Preferences): Promise<UpdateStatus> {
    if (!this.current.channels.some((c) => c.id === preferences.channel)) {
      return this.refuse({ kind: 'invalid', message: `There is no release channel '${preferences.channel}'.`, nextStep: 'Choose one of the options offered.', detail: null });
    }
    const changed = preferences.channel !== this.current.channel;
    const phase = changed && this.current.phase.kind !== 'unavailable' ? ({ kind: 'idle' } as const) : this.current.phase;
    return Promise.resolve(this.set(phase, { autoCheck: preferences.autoCheck, channel: preferences.channel }));
  }

  listen(handler: (status: UpdateStatus) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }
}
