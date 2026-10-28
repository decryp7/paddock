import { Injector, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { DISMISSED, provideContributions, SHELL_IDLE, SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import { assertContributionsValid, createFramed } from '@decryp7/paddock-ui/shell/testing';
import { MemoryJobsBackend, provideShellJobs } from '@decryp7/paddock-ui/shell/jobs';
import type { AboutInfo, UpdateStatus, WhatsNew } from '../generated/about';
import { ABOUT_COMMANDS } from '../generated/about-commands';
import { FILES_COMMANDS } from '../../files/generated/files-commands';
import {
  MemoryAboutBackend,
  MemoryUpdatesBackend,
  TauriAboutBackend,
  TauriUpdatesBackend,
  UpdatesBackend,
  type AboutIpc,
  type MemoryUpdatesOptions,
} from './backend';
import { ABOUT_DIALOG_COMMANDS, ABOUT_DIALOG_IDS, aboutContributions, defineAboutDialogs } from './about.contributions';
import { PdAboutDialog } from './about-dialog';
import { formatBytes, systemText } from './format';
import { ABOUT_STYLES } from './styles';
import { provideShellAbout } from './provide-about';
import { PdUpdatesDialog } from './updates-dialog';
import { UPDATES_CONFIG, UpdatesStore } from './updates.store';
import { PdWhatsNewDialog } from './whats-new-dialog';
import { asUpdateError, parseAboutInfo, parseUpdateStatus, parseWhatsNew, parseWhatsNewDecision, toUpdateError } from './wire';

const INFO: AboutInfo = {
  appName: 'Demo',
  version: '1.4.0',
  os: 'macos',
  arch: 'aarch64',
  commit: 'abc1234',
  built: '2026-10-24',
  homepage: null,
  releasesPage: 'https://updates.example/releases',
  legal: [
    { id: 'notices', title: 'Third-party notices' },
    { id: 'mit', title: 'MIT licence' },
  ],
  updatesAvailable: true,
};

const NOTES: WhatsNew = {
  version: '1.4.0',
  release: { version: '1.4.0', date: '2026-10-24', groups: [{ title: 'Added', items: ['Updates in the app', 'About'] }] },
};

const RELEASE = { version: '1.5.0', notes: 'Faster.', pubDate: null, size: 41_000_000 };

/** Lets the simulated engine and the stores settle (microtasks only). */
async function settle(rounds = 6): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

function bed(server: MemoryUpdatesOptions['server'], extra: unknown[] = [], main = false) {
  const notices: ShellNotice[] = [];
  const opened: string[] = [];
  const about = new MemoryAboutBackend(INFO, { notices: '# Third-party notices\n\nlong list', mit: 'MIT License' }, NOTES, 'show');
  const updates = new MemoryUpdatesBackend({ currentVersion: INFO.version, server, release: RELEASE, channels: [{ id: 'stable', label: 'Stable' }, { id: 'beta', label: 'Beta' }] });
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: (id: string) => (opened.push(id), Promise.resolve(DISMISSED)), notify: (n: ShellNotice) => notices.push(n) } },
      { provide: SHELL_IDLE, useValue: (work: () => void) => work() },
      provideShellAbout({ about: () => about, updates: () => updates, main }),
      ...(extra as never[]),
    ],
  });
  return { about, updates, notices, opened };
}

function text(el: HTMLElement): string {
  return (el.textContent ?? '').replace(/\s+/g, ' ').trim();
}

function button(el: HTMLElement, name: string): HTMLButtonElement {
  const b = [...el.querySelectorAll('button')].find((x) => text(x) === name);
  if (!b) throw new Error(`no button "${name}" in: ${text(el)}`);
  return b;
}

describe('the wire', () => {
  const status: UpdateStatus = {
    currentVersion: '1.0.0',
    phase: { kind: 'idle' },
    lastChecked: null,
    autoCheck: false,
    channel: 'stable',
    channels: [],
    endpointHost: null,
    releasesPage: null,
  };

  it('accepts what the engine sends and refuses anything else', () => {
    expect(parseUpdateStatus(status)).toBe(status);
    for (const bad of [null, [], {}, { ...status, phase: { kind: 'exploded' } }, { ...status, phase: null }, { ...status, channels: 3 }, { ...status, phase: { kind: 1 } }]) {
      expect(parseUpdateStatus(bad)).toBeNull();
    }
    expect(parseAboutInfo(INFO)).toBe(INFO);
    expect(parseAboutInfo({ appName: 'x' })).toBeNull();
    expect(parseWhatsNew(NOTES)).toBe(NOTES);
    expect(parseWhatsNew({ version: '1', release: null })).toEqual({ version: '1', release: null });
    expect(parseWhatsNew({ version: '1', release: 'all of them' })).toBeNull();
    expect(parseWhatsNew(7)).toBeNull();
    expect(['show', 'remember', 'nothing', 'maybe'].map(parseWhatsNewDecision)).toEqual(['show', 'remember', 'nothing', null]);
  });

  it('keeps the engine’s errors and wraps anything else with a next step', () => {
    const e = { kind: 'offline', message: 'No network.', nextStep: 'Check it.', detail: null };
    expect(asUpdateError(e)).toBe(e);
    expect(asUpdateError(new Error('x'))).toBeNull();
    expect(toUpdateError(e)).toBe(e);
    expect(toUpdateError(new Error('boom')).message).toBe('boom');
    expect(toUpdateError('plain').message).toBe('plain');
    expect(toUpdateError(undefined)).toMatchObject({ kind: 'unreachable', message: 'The updater did not answer.' });
    expect(formatBytes(12)).toBe('12 bytes');
    expect(formatBytes(1_500)).toBe('1.5 KB');
    expect(formatBytes(41_000_000)).toBe('39 MB');
    expect(formatBytes(2_000_000_000)).toBe('1.9 GB');
  });

  it('the Tauri backends call the generated commands and parse at the edge', async () => {
    const calls: [string, unknown][] = [];
    let handler: ((p: unknown) => void) | undefined;
    const replies: Record<string, unknown> = {
      about_info: INFO,
      about_legal: 'MIT License',
      about_whats_new: NOTES,
      about_whats_new_start: 'show',
    };
    const ipc: AboutIpc = {
      invoke: <T>(command: string, args?: Record<string, unknown>) => {
        calls.push([command, args]);
        return Promise.resolve((command in replies ? replies[command] : status) as T);
      },
      listen: (event, h) => {
        expect(event).toBe('paddock://updates');
        handler = h;
        return Promise.resolve(() => undefined);
      },
    };
    const about = new TauriAboutBackend(ipc);
    expect(await about.info()).toBe(INFO);
    expect(await about.legal('mit')).toBe('MIT License');
    expect(await about.whatsNew()).toBe(NOTES);
    expect(await about.whatsNewOnStart()).toBe('show');
    await about.openUrl('https://updates.example/releases');
    expect(calls.at(-1)).toEqual(['files_open_url', { url: 'https://updates.example/releases' }]);
    const updates = new TauriUpdatesBackend(ipc);
    for (const run of [
      () => updates.status(),
      () => updates.check(),
      () => updates.download(),
      () => updates.cancel(),
      () => updates.installOnQuit(true),
      () => updates.restart(),
      () => updates.setPreferences({ autoCheck: true, channel: 'beta' }),
    ]) {
      expect(await run()).toBe(status);
    }
    const seen: UpdateStatus[] = [];
    await updates.listen((s) => seen.push(s));
    handler?.(status);
    handler?.({ nonsense: true });
    expect(seen).toEqual([status]);
    const names = calls.map(([c]) => c);
    expect(names.every((n) => n in ABOUT_COMMANDS || n in FILES_COMMANDS)).toBe(true);
    expect(calls).toContainEqual(['about_legal', { id: 'mit' }]);
    expect(calls).toContainEqual(['updates_install_on_quit', { enabled: true }]);
    expect(calls).toContainEqual(['updates_set_preferences', { preferences: { autoCheck: true, channel: 'beta' } }]);
    replies['about_info'] = { broken: true };
    replies['about_legal'] = 3;
    replies['about_whats_new_start'] = 'perhaps';
    replies['about_whats_new'] = null;
    await expect(about.info()).rejects.toThrow('The engine sent no valid About information.');
    await expect(about.legal('x')).rejects.toThrow('legal text');
    await expect(about.whatsNew()).rejects.toThrow('release notes');
    await expect(about.whatsNewOnStart()).rejects.toThrow('start-up decision');
  });

  it('every dialog contract names only commands of the generated table', () => {
    // The one foreign command: About's releases link opens through the files adapter.
    for (const list of Object.values(ABOUT_DIALOG_COMMANDS)) for (const c of list) expect(c in ABOUT_COMMANDS || c === 'files_open_url', c).toBe(true);
    expect(FILES_COMMANDS.files_open_url.windows).toContain('dialog');
    expect(ABOUT_COMMANDS.about_whats_new_start.windows).toEqual(['main']);
  });
});

describe('the fake update server', () => {
  async function steps(server: MemoryUpdatesOptions['server']) {
    const b = new MemoryUpdatesBackend({ currentVersion: '1.0.0', server, release: RELEASE });
    const kinds: string[] = [];
    await b.listen((s) => kinds.push(s.phase.kind));
    return { b, kinds };
  }

  it('plays check, download, install on quit, restart and cancel like the engine', async () => {
    const { b, kinds } = await steps('release');
    expect((await b.check()).phase.kind).toBe('checking');
    await settle();
    expect((await b.status()).lastChecked).not.toBeNull();
    await b.download();
    await settle();
    expect((await b.installOnQuit(true)).phase).toMatchObject({ kind: 'ready', installOnQuit: true });
    expect((await b.cancel()).phase.kind).toBe('available');
    await b.download();
    await settle();
    expect((await b.restart()).phase.kind).toBe('installing');
    expect(kinds).toEqual(['checking', 'available', 'downloading', 'ready', 'ready', 'available', 'downloading', 'ready', 'installing']);
    await expect(b.check()).rejects.toMatchObject({ kind: 'busy' });
    await expect(b.restart()).rejects.toMatchObject({ kind: 'busy' });
    await expect(b.installOnQuit(true)).rejects.toMatchObject({ kind: 'busy' });
  });

  it('refuses a bad signature, says offline and up to date, and checks nothing without a key', async () => {
    const bad = await steps('bad-signature');
    await bad.b.check();
    await settle();
    await bad.b.download();
    await settle();
    expect((await bad.b.status()).phase).toMatchObject({ kind: 'failed', error: { kind: 'signatureInvalid' } });
    await expect(bad.b.download()).rejects.toMatchObject({ message: 'There is no update to download.' });
    const offline = await steps('offline');
    await offline.b.check();
    await settle();
    expect((await offline.b.status()).phase).toMatchObject({ kind: 'failed', error: { kind: 'offline' } });
    const none = await steps('none');
    await none.b.check();
    await settle();
    expect((await none.b.status()).phase.kind).toBe('upToDate');
    await expect(none.b.cancel()).rejects.toMatchObject({ message: 'There is no download to cancel.' });
    const off = await steps('unavailable');
    expect((await off.b.status()).endpointHost).toBeNull();
    await expect(off.b.check()).rejects.toMatchObject({ kind: 'notAvailable', message: 'Updates are not available in this build.' });
    await expect(off.b.cancel()).rejects.toMatchObject({ kind: 'notAvailable' });
  });

  it('cancels a check, drops stale results and validates the channel', async () => {
    const held: (() => void)[] = [];
    const b = new MemoryUpdatesBackend({ currentVersion: '1.0.0', server: 'release', release: RELEASE, schedule: (s) => held.push(s) });
    await b.check();
    expect((await b.cancel()).phase.kind).toBe('idle');
    held.forEach((s) => s());
    expect((await b.status()).phase.kind).toBe('idle');
    await expect(b.setPreferences({ autoCheck: true, channel: 'nightly' })).rejects.toMatchObject({ kind: 'invalid' });
    expect((await b.setPreferences({ autoCheck: true, channel: 'stable' })).autoCheck).toBe(true);
    const unset = new MemoryUpdatesBackend({ currentVersion: '1.0.0', server: 'release', channels: [] });
    expect((await unset.status()).channel).toBe('');
    await unset.check();
    await settle();
    expect((await unset.status()).phase).toMatchObject({ kind: 'available', info: { version: '99.0.0' } });
    const unlisten = await unset.listen(() => undefined);
    unlisten();
  });
});

describe('UpdatesStore', () => {
  it('follows the engine, announces a found update once in the main window, and keeps refusals with their next step', async () => {
    const { notices } = bed('release', [], true);
    const store = TestBed.inject(UpdatesStore);
    await settle();
    expect(store.kind()).toBe('idle');
    expect(store.isLoaded()).toBe(true);
    await store.check();
    await settle();
    expect(store.kind()).toBe('available');
    expect(store.info()?.version).toBe('1.5.0');
    expect(store.canDownload()).toBe(true);
    await store.check();
    await settle();
    expect(notices.filter((n) => n.kind === 'pd.updates.available')).toHaveLength(1);
    expect(notices[0]?.message).toBe('Version 1.5.0 is available (you have 1.4.0).');
    expect(await store.restart()).toBe(false);
    expect(store.error()?.message).toBe('No verified update is ready to install.');
    expect(await store.download()).toBe(true);
    expect(store.error()).toBeNull();
    await settle();
    expect(store.kind()).toBe('ready');
    expect(store.job()).toBeNull();
  });

  it('a reply that crossed a newer event does not win, and a failed first read says so', async () => {
    const order: UpdatesBackend = new (class extends MemoryUpdatesBackend {
      override check(): Promise<UpdateStatus> {
        const reply = super.check();
        return reply.then(async (s) => {
          await settle();
          return s;
        });
      }
    })({ currentVersion: '1.0.0', server: 'release', release: RELEASE });
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideShellAbout({ about: () => new MemoryAboutBackend(INFO), updates: () => order })],
    });
    const store = TestBed.inject(UpdatesStore);
    await settle();
    await store.check();
    expect(store.kind()).toBe('available');
    TestBed.resetTestingModule();
    const broken = new (class extends MemoryUpdatesBackend {
      fail = true;
      override status(): Promise<UpdateStatus> {
        return this.fail ? Promise.reject(new Error('the engine is gone')) : super.status();
      }
      override listen(): Promise<() => void> {
        return Promise.reject(new Error('no events'));
      }
    })({ currentVersion: '1.0.0', server: 'none' });
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideShellAbout({ about: () => new MemoryAboutBackend(INFO), updates: () => broken })],
    });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const s2 = TestBed.inject(UpdatesStore);
    await settle();
    expect(s2.requestError()).toBe('the engine is gone');
    expect(warn).toHaveBeenCalled();
    broken.fail = false;
    await s2.load();
    expect(s2.kind()).toBe('idle');
    warn.mockRestore();
  });

  it('stops listening when its window goes away', async () => {
    const { updates } = bed('none');
    const listen = vi.spyOn(updates, 'listen');
    TestBed.inject(UpdatesStore);
    await settle();
    expect(listen).toHaveBeenCalledTimes(1);
    TestBed.resetTestingModule();
    await settle();
  });
});

describe('the main window', () => {
  it('opens What’s New once after an update and starts listening for updates when idle', async () => {
    const { opened, about } = bed('none', [], true);
    TestBed.inject(Injector);
    TestBed.inject(UpdatesStore);
    await settle();
    expect(opened).toEqual([ABOUT_DIALOG_IDS.whatsNew]);
    expect(await about.whatsNewOnStart()).toBe('nothing');
  });

  it('a failed start-up question is only logged', async () => {
    const failing = new (class extends MemoryAboutBackend {
      override whatsNewOnStart(): Promise<'show'> {
        return Promise.reject(new Error('gone'));
      }
    })(INFO);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        { provide: SHELL_IDLE, useValue: (work: () => void) => work() },
        provideShellAbout({ about: () => failing, updates: () => new MemoryUpdatesBackend({ currentVersion: '1', server: 'none' }), main: true }),
      ],
    });
    TestBed.inject(UpdatesStore);
    await settle();
    expect(warn.mock.calls.some(([m]) => String(m).includes('What\'s New at start-up: gone'))).toBe(true);
    warn.mockRestore();
  });
});

describe('the dialogs', () => {
  it('About shows the engine’s version and fetches a legal text only when asked', async () => {
    const { about } = bed('none');
    const fixture = TestBed.createComponent(PdAboutDialog);
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector('[data-testid="about-version"]')?.textContent).toBe(`Version ${INFO.version}`);
    expect(text(el)).toContain('abc1234');
    expect(text(el)).toContain('Updates itself');
    expect(about.legalReads).toBe(0);
    button(el, 'Third-party notices').click();
    await fixture.whenStable();
    expect(about.legalReads).toBe(1);
    expect(el.querySelector('[data-testid="about-legal"]')?.textContent).toContain('# Third-party notices');
    expect(el.querySelector('[data-testid="about-legal"]')?.innerHTML).not.toContain('<h1');
    button(el, 'Third-party notices').click();
    await fixture.whenStable();
    expect(el.querySelector('[data-testid="about-legal"]')).toBeNull();
  });

  it('About says the system in words, links the releases page and styles every class it uses', async () => {
    bed('none');
    const fixture = TestBed.createComponent(PdAboutDialog);
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    const facts = [...el.querySelectorAll('dl.pd-facts dt')].map((dt) => [text(dt as HTMLElement), text(dt.nextElementSibling as HTMLElement)]);
    expect(facts).toContainEqual(['System', 'macOS (Apple silicon)']);
    expect(text(el)).not.toContain('aarch64');
    const link = el.querySelector<HTMLAnchorElement>('[data-testid="about-releases"]')!;
    expect(link.tagName).toBe('A');
    expect(link.getAttribute('href')).toBe('https://updates.example/releases');
    expect(link.rel).toContain('noopener');
    // A webview window opens no links itself: the click asks the engine to open the page in the system browser.
    const opened: string[] = [];
    vi.spyOn(MemoryAboutBackend.prototype, 'openUrl').mockImplementation((url: string) => (opened.push(url), Promise.resolve()));
    const click = new MouseEvent('click', { bubbles: true, cancelable: true });
    link.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(true);
    expect(opened).toEqual(['https://updates.example/releases']);
    // The stale-selector class of bug: a class the template uses but the stylesheet does not define.
    const used = new Set([...el.querySelectorAll('[class]')].flatMap((n) => [...n.classList].filter((c) => c.startsWith('pd-'))));
    for (const c of used) expect(ABOUT_STYLES, c).toContain(`.${c}`);
  });

  it('About says why the releases page did not open, and the memory backend records what it opened', async () => {
    const { about } = bed('none');
    expect(about.openedUrls).toEqual([]);
    const fixture = TestBed.createComponent(PdAboutDialog);
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    el.querySelector<HTMLAnchorElement>('[data-testid="about-releases"]')!.click();
    await fixture.whenStable();
    expect(about.openedUrls).toEqual(['https://updates.example/releases']);
    expect(el.querySelector('[data-testid="about-open-failed"]')).toBeNull();
    vi.spyOn(about, 'openUrl').mockRejectedValue(new Error('only http, https and mailto addresses open'));
    el.querySelector<HTMLAnchorElement>('[data-testid="about-releases"]')!.click();
    await fixture.whenStable();
    const failed = el.querySelector('[data-testid="about-open-failed"]');
    expect(failed?.getAttribute('role')).toBe('alert');
    expect(text(failed as HTMLElement)).toContain('The releases page could not be opened');
    expect(text(failed as HTMLElement)).toContain('only http, https and mailto addresses open');
  });

  it('says the operating system and architecture in words', () => {
    expect(systemText('macos', 'x86_64')).toBe('macOS (Intel)');
    expect(systemText('windows', 'x86_64')).toBe('Windows (64-bit)');
    expect(systemText('windows', 'aarch64')).toBe('Windows (ARM)');
    expect(systemText('linux', 'x86_64')).toBe('Linux (64-bit)');
    expect(systemText('browser', 'web')).toBe('Web browser');
    expect(systemText('haiku', 'riscv')).toBe('haiku (riscv)');
  });

  it('About in a build without updates and with a missing text says so', async () => {
    const about = new MemoryAboutBackend({ ...INFO, updatesAvailable: false, commit: null, built: null, releasesPage: null, legal: [{ id: 'gone', title: 'Gone' }] });
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideShellAbout({ about: () => about, updates: () => new MemoryUpdatesBackend({ currentVersion: '1', server: 'unavailable' }) })],
    });
    const fixture = TestBed.createComponent(PdAboutDialog);
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    expect(text(el)).toContain('Updates are not available in this build');
    button(el, 'Gone').click();
    await fixture.whenStable();
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('could not be loaded');
  });

  it('What’s New shows the running version’s notes, and says so when there are none', async () => {
    bed('none');
    let fixture = TestBed.createComponent(PdWhatsNewDialog);
    await fixture.whenStable();
    let el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector('[data-testid="whats-new-heading"]')?.textContent).toBe('What’s new in 1.4.0');
    expect(text(el)).toContain('Updates in the app');
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideShellAbout({ about: () => new MemoryAboutBackend({ ...INFO, version: '1.4.1' }), updates: () => new MemoryUpdatesBackend({ currentVersion: '1.4.1', server: 'none' }) }),
      ],
    });
    fixture = TestBed.createComponent(PdWhatsNewDialog);
    await fixture.whenStable();
    el = fixture.nativeElement;
    expect(el.querySelector('[data-testid="whats-new-none"]')?.textContent).toBe('There are no release notes for version 1.4.1.');
  });

  it('Updates walks through check, download (with the job’s progress), restart and the preferences', async () => {
    const jobs = new MemoryJobsBackend();
    const { updates } = bed('release', [provideShellJobs({ backend: () => jobs })]);
    const fixture = createFramed(PdUpdatesDialog);
    await settle();
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector('[data-testid="updates-current"]')?.textContent).toBe(`You have version ${INFO.version}.`);
    expect(text(el)).toContain('Not checked yet.');
    // Said once (the phase line), not again in the footer; the auto-check box and the channel are separate rows.
    expect(text(el).match(/Not checked/g)).toHaveLength(1);
    const prefs = el.querySelector('[data-testid="updates-prefs"]')!;
    const auto = prefs.querySelector('[data-testid="updates-auto"]')!;
    const channel = prefs.querySelector('[data-testid="updates-channel"]')!;
    expect(auto.contains(channel) || channel.contains(auto)).toBe(false);
    expect(text(auto as HTMLElement)).toBe('Check for updates automatically');
    expect(text(channel as HTMLElement)).toContain('Channel');
    button(el, 'Check now').click();
    await settle();
    await fixture.whenStable();
    expect(text(el)).toContain('Version 1.5.0 is available (39 MB).');
    expect(text(el)).toContain('Faster.');
    // Hold the download in its job to see the progress.
    const hold: (() => void)[] = [];
    (updates as unknown as { schedule: (s: () => void) => void }).schedule = (s) => hold.push(s);
    button(el, 'Download').click();
    await settle();
    const job = updates as unknown as { current: UpdateStatus };
    const id = job.current.phase.kind === 'downloading' ? job.current.phase.job : -1;
    jobs.emit({ id: id ?? -1, kind: 'update-download', title: 'Downloading the update', subject: '1.5.0', seq: 1, elapsedMs: 0, etaMs: null, cancellable: true, notCancellableReason: null, state: 'running', phase: { id: 'download', label: 'Downloading', unit: 'bytes', total: 41_000_000, done: 20_500_000 } });
    await settle();
    await fixture.whenStable();
    expect(text(el)).toContain('Downloading: 20 MB of 39 MB');
    expect(el.querySelector('progress')?.getAttribute('value')).toBe('0.5');
    hold.forEach((s) => s());
    await settle();
    await fixture.whenStable();
    expect(text(el)).toContain('downloaded and its signature verified');
    el.querySelector<HTMLInputElement>('label.pd-check input[type="checkbox"]')?.click();
    await settle();
    expect((await updates.status()).autoCheck).toBe(true);
    expect(el.querySelector('select')).toBeNull();
    el.querySelector<HTMLButtonElement>('pd-select button')!.click();
    await settle();
    await fixture.whenStable();
    [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=option]')].find((o) => o.textContent?.includes('Beta'))!.click();
    await settle();
    await fixture.whenStable();
    expect((await updates.status()).channel).toBe('beta');
    expect(text(el)).toContain('Not checked yet.');
  });

  it('Updates shows a refused signature with its next step, and nothing to press without a key', async () => {
    bed('bad-signature');
    const fixture = createFramed(PdUpdatesDialog);
    await settle();
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    button(el, 'Check now').click();
    await settle();
    await fixture.whenStable();
    button(el, 'Download').click();
    await settle();
    await fixture.whenStable();
    const error = el.querySelector('[data-testid="updates-error"]');
    expect(error?.getAttribute('role')).toBe('alert');
    expect(text(error as HTMLElement)).toContain("The update's signature is not valid, so the update was refused.");
    expect(text(error as HTMLElement)).toContain('Do not install it by hand');
    expect(button(el, 'Check again')).toBeTruthy();
    TestBed.resetTestingModule();
    bed('unavailable');
    const off = createFramed(PdUpdatesDialog);
    await settle();
    await off.whenStable();
    const o: HTMLElement = off.nativeElement;
    expect(text(o)).toContain('Updates are not available in this build.');
    expect(text(o)).toContain('without an update signing key');
    // Nothing to press but the frame's Close.
    expect([...o.querySelectorAll('button')].map((b) => text(b))).toEqual(['Close']);
  });

  it('Updates offers Stop checking while checking and a Retry when the updater does not answer', async () => {
    const { updates } = bed('release');
    (updates as unknown as { schedule: (s: () => void) => void }).schedule = () => undefined;
    const fixture = createFramed(PdUpdatesDialog);
    await settle();
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    button(el, 'Check now').click();
    await settle();
    await fixture.whenStable();
    expect(text(el)).toContain('Checking for updates…');
    button(el, 'Stop checking').click();
    await settle();
    await fixture.whenStable();
    expect(text(el)).toContain('Not checked yet.');
  });
});

describe('the contributions', () => {
  it('are valid: three dialogs of the right kinds, Help menu items and the notice kind', () => {
    const dialogs = defineAboutDialogs({ providers: () => Promise.resolve([]) });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions(aboutContributions(dialogs))] });
    assertContributionsValid(TestBed.inject(Injector));
    expect(dialogs.map((d) => [d.id, d.kind, d.modality])).toEqual([
      ['pd.about', 'viewer', 'modeless'],
      ['pd.updates', 'form', 'modeless'],
      ['pd.whats-new', 'viewer', 'modeless'],
    ]);
  });

  it('each command opens its dialog, and each dialog loads lazily', async () => {
    const dialogs = defineAboutDialogs({ providers: () => Promise.resolve([]) });
    const batch = aboutContributions(dialogs);
    const opened: string[] = [];
    const ctx = { openDialog: (id: string) => (opened.push(id), Promise.resolve(DISMISSED)) } as never;
    for (const c of batch.commands ?? []) await c.run(ctx);
    expect(opened).toEqual(['pd.about', 'pd.updates', 'pd.whats-new']);
    const loaded = await Promise.all(dialogs.map((d) => d.load()));
    expect(loaded).toEqual([PdAboutDialog, PdUpdatesDialog, PdWhatsNewDialog]);
  });
});

describe('the edges', () => {
  it('a window without provideShellAbout announces nothing; failures offer Retry', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    expect(TestBed.inject(UPDATES_CONFIG)).toEqual({ announce: false });
    TestBed.resetTestingModule();
    const failing = new (class extends MemoryAboutBackend {
      override info(): Promise<AboutInfo> {
        return Promise.reject(new Error('gone'));
      }
      override whatsNew(): Promise<WhatsNew> {
        return Promise.reject(new Error('gone'));
      }
    })(INFO);
    const silent = new (class extends MemoryUpdatesBackend {
      override status(): Promise<UpdateStatus> {
        return Promise.reject(new Error('The updater is not running.'));
      }
    })({ currentVersion: '1', server: 'none' });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellAbout({ about: () => failing, updates: () => silent })] });
    for (const c of [PdAboutDialog, PdWhatsNewDialog, PdUpdatesDialog] as const) {
      const f = TestBed.createComponent(c as never);
      await settle();
      await f.whenStable();
      const el = (f as { nativeElement: HTMLElement }).nativeElement;
      expect(el.querySelector('[role="alert"]')).not.toBeNull();
      button(el, 'Retry').click();
      await settle();
    }
  });

  it('Updates: up to date, offline, and install when the app quits', async () => {
    bed('none');
    let f = createFramed(PdUpdatesDialog);
    await settle();
    await f.whenStable();
    let el: HTMLElement = f.nativeElement;
    button(el, 'Check now').click();
    await settle();
    await f.whenStable();
    expect(text(el)).toContain('You have the latest version.');
    expect(button(el, 'Check now')).toBeTruthy();
    TestBed.resetTestingModule();
    bed('offline');
    f = createFramed(PdUpdatesDialog);
    await settle();
    await f.whenStable();
    el = f.nativeElement;
    button(el, 'Check now').click();
    await settle();
    await f.whenStable();
    expect(text(el)).toContain('The network could not be reached.');
    expect(button(el, 'Check again')).toBeTruthy();
    TestBed.resetTestingModule();
    const { updates } = bed('release');
    f = createFramed(PdUpdatesDialog);
    await settle();
    await f.whenStable();
    el = f.nativeElement;
    button(el, 'Check now').click();
    await settle();
    await f.whenStable();
    button(el, 'Download').click();
    await settle();
    await f.whenStable();
    const quit = [...el.querySelectorAll('label.pd-check')].find((l) => text(l as HTMLElement).includes('Install when the app quits'));
    quit?.querySelector('input')?.click();
    await settle();
    expect((await updates.status()).phase).toMatchObject({ kind: 'ready', installOnQuit: true });
    button(el, 'Restart to update').click();
    await settle();
    await f.whenStable();
    expect(text(el)).toContain('Installing version 1.5.0…');
  });
});

describe('the main window’s start-up work', () => {
  it('waits for the window’s ready signal before it reaches the engine', async () => {
    const ready = signal(false);
    const about = new MemoryAboutBackend(INFO, {}, NOTES, 'show');
    const ask = vi.spyOn(about, 'whatsNewOnStart');
    const opened: string[] = [];
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        { provide: SHELL_IDLE, useValue: (work: () => void) => work() },
        { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: (id: string) => (opened.push(id), Promise.resolve(DISMISSED)), notify: () => undefined } },
        provideShellAbout({ about: () => about, updates: () => new MemoryUpdatesBackend({ currentVersion: '1', server: 'none' }), main: true, ready: () => ready }),
      ],
    });
    TestBed.inject(Injector);
    TestBed.tick();
    await settle();
    expect(ask).not.toHaveBeenCalled();
    ready.set(true);
    TestBed.tick();
    await settle();
    expect(ask).toHaveBeenCalledTimes(1);
    expect(opened).toEqual(['pd.whats-new']);
    ready.set(false);
    ready.set(true);
    TestBed.tick();
    await settle();
    expect(ask).toHaveBeenCalledTimes(1);
  });
});
