import { ErrorHandler, Injector, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { DISMISSED, provideContributions } from '@decryp7/paddock-ui/shell';
import { DIALOG_REF } from '@decryp7/paddock-ui/shell/dialogs';
import { assertContributionsValid, createFramed } from '@decryp7/paddock-ui/shell/testing';
import type { Preview, PreviewReply, RedactionCounts, Saved, SaveOutcome } from '../generated/diagnostics';
import { DIAGNOSTICS_COMMANDS } from '../generated/diagnostics-commands';
import { DiagnosticsBackend, MemoryDiagnosticsBackend, TauriDiagnosticsBackend, type DiagnosticsIpc } from './backend';
import { DiagnosticsStore, redactionSummary } from './diagnostics.store';
import { DIAGNOSTICS_DIALOG_COMMANDS, DIAGNOSTICS_DIALOG_ID, LOGGING_SETTINGS_PAGE, defineDiagnosticsDialog, diagnosticsContributions } from './diagnostics.contributions';
import { formatBytes, truncateMiddle } from './format';
import { provideShellDiagnostics } from './provide-diagnostics';
import { PdReportProblemDialog } from './report-problem-dialog';
import { DIAGNOSTICS_STYLES } from './styles';
import { MAX_FORWARDED_LINES, MAX_FORWARDED_MESSAGE, WebviewLog, provideWebviewLog, type WebviewLogIpc } from './webview-log';
import { asDiagError, diagMessage, parsePreviewReply, parseSaveOutcome } from './wire';

const FILES: Preview['files'] = [
  { name: 'system-info.json', kind: 'system-info', bytes: 412, summary: 'Demo 1.0.0, your operating system and processor type' },
  { name: 'logs/demo.2026-10-24.log', kind: 'log', bytes: 182_400, summary: '1,204 log lines' },
];

/** Lets the simulated engine and the stores settle (microtasks only). */
async function settle(rounds = 8): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

function text(el: HTMLElement): string {
  return (el.textContent ?? '').replace(/\s+/g, ' ').trim();
}

function button(el: HTMLElement, name: string): HTMLButtonElement {
  const b = [...el.querySelectorAll('button')].find((x) => text(x) === name);
  if (!b) throw new Error(`no button "${name}" in: ${text(el)}`);
  return b;
}

function bed(options: Partial<ConstructorParameters<typeof MemoryDiagnosticsBackend>[0]> = {}, backend?: DiagnosticsBackend) {
  // The engine's steps wait until `flush()`: a test sees "saving" before the engine answers.
  const held: (() => void)[] = [];
  const memory = new MemoryDiagnosticsBackend({ files: FILES, schedule: (step) => held.push(step), ...options });
  const flush = async (): Promise<void> => {
    held.splice(0).forEach((step) => step());
    await settle();
  };
  const closed: (Saved | undefined)[] = [];
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      provideShellDiagnostics({ backend: () => backend ?? memory }),
      { provide: DIALOG_REF, useValue: { answer: (r?: Saved) => closed.push(r), dismiss: () => closed.push(undefined), closed: Promise.resolve(DISMISSED) } },
    ],
  });
  return { memory, closed, flush };
}

const COUNTS: RedactionCounts = { secrets: 0, fields: 0, homePaths: 0, profilePaths: 0, machineNames: 0, userNames: 0, emails: 0, ipAddresses: 0 };
const PREVIEW: Preview = { files: FILES, totalBytes: 182_812, redactions: COUNTS, notes: [] };
const REPLY: PreviewReply = { token: 'r1', preview: PREVIEW };

describe('the wire', () => {
  it('accepts what the engine sends and refuses anything else', () => {
    expect(parsePreviewReply(REPLY)).toBe(REPLY);
    for (const bad of [null, [], {}, { token: 1, preview: PREVIEW }, { token: 'r', preview: null }, { token: 'r', preview: { ...PREVIEW, files: 'x' } }, { token: 'r', preview: { ...PREVIEW, files: [{ name: 1 }] } }, { token: 'r', preview: { ...PREVIEW, notes: 3 } }]) {
      expect(parsePreviewReply(bad)).toBeNull();
    }
    const saved = { state: 'saved', path: '/a/b.zip', bytes: 10, files: 3 };
    expect(parseSaveOutcome(saved)).toBe(saved);
    expect(parseSaveOutcome({ state: 'dismissed' })).toEqual({ state: 'dismissed' });
    expect(parseSaveOutcome({ state: 'cancelled', extra: 1 })).toEqual({ state: 'cancelled' });
    expect(parseSaveOutcome({ state: 'failed', message: 'full' })).toEqual({ state: 'failed', message: 'full' });
    for (const bad of [null, 'saved', { state: 'saved' }, { state: 'failed' }, { state: 'exploded' }, []]) expect(parseSaveOutcome(bad)).toBeNull();
  });

  it('keeps the engine’s refusals and wraps anything else with a message', () => {
    const refusal = { code: 'expired', message: 'Out of date.', retryable: true };
    expect(asDiagError(refusal)).toBe(refusal);
    expect(asDiagError({ code: 'weird', message: 'x' })).toBeNull();
    expect(asDiagError(new Error('x'))).toBeNull();
    expect(diagMessage(refusal, 'f')).toBe('Out of date.');
    expect(diagMessage(new Error('boom'), 'f')).toBe('boom');
    expect(diagMessage('plain', 'f')).toBe('plain');
    expect(diagMessage(undefined, 'fallback')).toBe('fallback');
  });

  it('formats sizes and cuts long paths in the middle', () => {
    expect(formatBytes(12)).toBe('12 bytes');
    expect(formatBytes(1_500)).toBe('1.5 KB');
    expect(formatBytes(182_400)).toBe('182 KB');
    expect(formatBytes(41_000_000)).toBe('41 MB');
    expect(formatBytes(2_000_000_000)).toBe('2 GB');
    expect(truncateMiddle('short.zip', 20)).toBe('short.zip');
    const long = '/Users/someone/Library/Application Support/Demo/reports/demo-diagnostics-20261024-103000.zip';
    const cut = truncateMiddle(long, 40);
    expect([...cut]).toHaveLength(40);
    expect(cut.startsWith('/Users/someone')).toBe(true);
    expect(cut.endsWith('103000.zip')).toBe(true);
    expect(cut).toContain('…');
    expect(truncateMiddle('abcdef', 2)).toBe('abcdef');
  });

  it('names what redaction removed in words, leaving out what is zero', () => {
    expect(redactionSummary(COUNTS)).toEqual([]);
    expect(redactionSummary({ ...COUNTS, secrets: 1, userNames: 3, homePaths: 1, profilePaths: 1, emails: 1, ipAddresses: 2, machineNames: 1, fields: 2 })).toEqual([
      '1 secret',
      '2 hidden values',
      '3 user names',
      '2 home folders',
      '1 computer name',
      '1 e-mail address',
      '2 IP addresses',
    ]);
  });
});

describe('the Tauri backend', () => {
  it('calls the generated commands and parses at the edge', async () => {
    const calls: [string, unknown][] = [];
    let handler: ((p: unknown) => void) | undefined;
    const replies: Record<string, unknown> = { diagnostics_preview: REPLY, diagnostics_save: null, diagnostics_cancel: true };
    const ipc: DiagnosticsIpc = {
      invoke: <T>(command: string, args?: Record<string, unknown>) => {
        calls.push([command, args]);
        return Promise.resolve(replies[command] as T);
      },
      listen: (event, h) => {
        expect(event).toBe('paddock://diagnostics');
        handler = h;
        return Promise.resolve(() => undefined);
      },
    };
    const backend = new TauriDiagnosticsBackend(ipc);
    expect(await backend.preview(null, 'it broke')).toBe(REPLY);
    expect(await backend.preview('r1', '')).toBe(REPLY);
    await backend.save('r1', 'it broke');
    expect(await backend.cancel()).toBe(true);
    replies['diagnostics_cancel'] = false;
    expect(await backend.cancel()).toBe(false);
    const seen: SaveOutcome[] = [];
    await backend.listen((o) => seen.push(o));
    handler?.({ state: 'dismissed' });
    handler?.({ nonsense: true });
    expect(seen).toEqual([{ state: 'dismissed' }]);
    expect(calls).toContainEqual(['diagnostics_preview', { token: null, description: 'it broke' }]);
    expect(calls).toContainEqual(['diagnostics_preview', { token: 'r1', description: '' }]);
    expect(calls).toContainEqual(['diagnostics_save', { token: 'r1', description: 'it broke' }]);
    expect(calls.every(([c]) => c in DIAGNOSTICS_COMMANDS)).toBe(true);
    replies['diagnostics_preview'] = { broken: true };
    await expect(backend.preview(null, '')).rejects.toThrow('The engine sent no valid report preview.');
  });

  it('every dialog command is in the generated table, and the report commands are the dialog’s alone', () => {
    for (const c of DIAGNOSTICS_DIALOG_COMMANDS) expect(c in DIAGNOSTICS_COMMANDS).toBe(true);
    expect(DIAGNOSTICS_COMMANDS.diagnostics_save.windows).toEqual(['dialog']);
    expect(DIAGNOSTICS_COMMANDS.log_from_webview.windows).toEqual(['main', 'popout', 'dialog', 'tray']);
  });
});

describe('the fake engine', () => {
  it('collects once per token, re-assembles with the description and refuses a stale token', async () => {
    const memory = new MemoryDiagnosticsBackend({ files: FILES });
    const first = await memory.preview(null, '');
    expect(first.preview.files.map((f) => f.name)).toEqual(['manifest.json', 'description.txt', 'system-info.json', 'logs/demo.2026-10-24.log']);
    const again = await memory.preview(first.token, 'it broke');
    expect(again.token).toBe(first.token);
    expect(memory.collections).toBe(1);
    expect(again.preview.files[1]).toMatchObject({ name: 'description.txt', bytes: 8, summary: 'Your description (8 characters)' });
    expect((await memory.preview('stale', '')).token).not.toBe(first.token);
    expect(memory.collections).toBe(2);
    await expect(memory.save(first.token, '')).rejects.toMatchObject({ code: 'expired' });
    expect(await memory.cancel()).toBe(false);
  });

  it('plays save, dismiss, fail and a hanging save that cancel stops', async () => {
    for (const [mode, state] of [['save', 'saved'], ['dismiss', 'dismissed'], ['fail', 'failed']] as const) {
      const held: (() => void)[] = [];
      const memory = new MemoryDiagnosticsBackend({ files: FILES, save: mode, schedule: (step) => held.push(step) });
      const outcomes: SaveOutcome[] = [];
      await memory.listen((o) => outcomes.push(o));
      const { token } = await memory.preview(null, '');
      await memory.save(token, '');
      await expect(memory.save(token, '')).rejects.toMatchObject({ code: 'busy' });
      held.forEach((step) => step());
      expect(outcomes.map((o) => o.state)).toEqual([state]);
    }
    const hang = new MemoryDiagnosticsBackend({ files: FILES, save: 'hang' });
    const outcomes: SaveOutcome[] = [];
    const off = await hang.listen((o) => outcomes.push(o));
    const { token } = await hang.preview(null, '');
    await hang.save(token, '');
    await settle();
    expect(outcomes).toEqual([]);
    expect(await hang.cancel()).toBe(true);
    await settle();
    expect(outcomes).toEqual([{ state: 'cancelled' }]);
    off();
  });
});

describe('DiagnosticsStore', () => {
  it('collects when its window opens, follows the description and saves through the engine', async () => {
    const { memory, flush } = bed();
    const store = TestBed.inject(DiagnosticsStore);
    await settle();
    expect(store.isLoaded()).toBe(true);
    expect(store.files().map((f) => f.name)).toContain('logs/demo.2026-10-24.log');
    expect(store.totalBytes()).toBeGreaterThan(182_000);
    store.setDescription('it broke');
    await store.commitDescription();
    expect(memory.collections).toBe(1);
    expect(store.files().find((f) => f.name === 'description.txt')?.bytes).toBe(8);
    await store.save();
    expect(store.saving()).toBe(true);
    await store.save();
    await flush();
    expect(store.saveState()).toMatchObject({ kind: 'saved', saved: { files: 4 } });
  });

  it('keeps each outcome, lets the user try again, and cancels', async () => {
    const { memory, flush } = bed({ save: 'hang' });
    // The engine's events, as the store hears them.
    let engine: ((o: SaveOutcome) => void) | undefined;
    const listen = memory.listen.bind(memory);
    vi.spyOn(memory, 'listen').mockImplementation((h) => ((engine = h), listen(h)));
    const store = TestBed.inject(DiagnosticsStore);
    await settle();
    await store.save();
    await store.cancel();
    await flush();
    expect(store.saveState()).toEqual({ kind: 'cancelled' });
    store.acknowledge();
    expect(store.saveState()).toEqual({ kind: 'idle' });
    engine?.({ state: 'failed', message: 'No space left on device.' });
    expect(store.saveState()).toEqual({ kind: 'failed', message: 'No space left on device.' });
    engine?.({ state: 'dismissed' });
    expect(store.saveState().kind).toBe('dismissed');
    expect(memory.collections).toBe(1);
  });

  it('shows a new preview when the engine says the old one is gone, and says why nothing was saved', async () => {
    const { memory } = bed();
    const store = TestBed.inject(DiagnosticsStore);
    await settle();
    const first = store.reply()?.token;
    vi.spyOn(memory, 'save').mockRejectedValueOnce({ code: 'expired', message: 'The preview is out of date.', retryable: true });
    await store.save();
    expect(store.saveState()).toEqual({ kind: 'failed', message: 'The preview is out of date.' });
    expect(store.reply()?.token).not.toBe(first);
    vi.spyOn(memory, 'save').mockRejectedValueOnce(new Error('no way to ask'));
    await store.save();
    expect(store.saveState()).toEqual({ kind: 'failed', message: 'no way to ask' });
    vi.spyOn(memory, 'cancel').mockRejectedValueOnce('cannot');
    await store.cancel();
    expect(store.saveState()).toEqual({ kind: 'failed', message: 'cannot' });
  });

  it('says so when the report cannot be put together, retries, and keeps the list when a refresh fails', async () => {
    const { memory } = bed();
    const preview = vi.spyOn(memory, 'preview').mockRejectedValueOnce(new Error('the engine is gone'));
    const store = TestBed.inject(DiagnosticsStore);
    await settle();
    expect(store.requestError()).toBe('the engine is gone');
    expect(store.files()).toEqual([]);
    await store.load();
    expect(store.isLoaded()).toBe(true);
    preview.mockRejectedValueOnce(new Error('busy'));
    await store.commitDescription();
    expect(store.requestError()).toBe('busy');
    expect(store.files().length).toBeGreaterThan(0);
    // Nothing is saved while there is no preview.
    TestBed.resetTestingModule();
    const { memory: m2 } = bed();
    vi.spyOn(m2, 'preview').mockRejectedValue(new Error('down'));
    const s2 = TestBed.inject(DiagnosticsStore);
    await settle();
    await s2.save();
    expect(s2.saveState()).toEqual({ kind: 'idle' });
  });

  it('stops listening when its window goes away, and says when it cannot listen', async () => {
    const { memory } = bed();
    const off = vi.fn();
    vi.spyOn(memory, 'listen').mockResolvedValue(off);
    TestBed.inject(DiagnosticsStore);
    await settle();
    TestBed.resetTestingModule();
    await settle();
    expect(off).toHaveBeenCalledTimes(1);
    const { memory: m2 } = bed();
    vi.spyOn(m2, 'listen').mockRejectedValue(new Error('no events'));
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    TestBed.inject(DiagnosticsStore);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    warn.mockRestore();
  });
});

describe('the Report a Problem dialog', () => {
  async function open(options: Parameters<typeof bed>[0] = {}) {
    const rig = bed(options);
    const fixture = createFramed(PdReportProblemDialog, { kind: "form" });
    await fixture.whenStable();
    await settle();
    await fixture.whenStable();
    return { ...rig, fixture, el: fixture.nativeElement as HTMLElement, answer: async () => (await rig.flush(), fixture.whenStable()) };
  }

  it('lists every file with its size before anything can be saved, and says nothing is sent', async () => {
    const { el } = await open();
    expect(text(el)).toContain('Nothing is sent anywhere.');
    const rows = [...el.querySelectorAll('[data-testid="diag-files"] tbody tr')].map((r) => text(r as HTMLElement));
    expect(rows).toHaveLength(4);
    expect(rows[0]).toContain('manifest.json');
    expect(rows[3]).toContain('logs/demo.2026-10-24.log');
    expect(rows[3]).toContain('182 KB');
    expect(text(el.querySelector('[data-testid="diag-total"]') as HTMLElement)).toMatch(/^4 files, .* before compression$/);
    expect(el.querySelector('table')?.querySelectorAll('th[scope="col"]')).toHaveLength(3);
    expect((el.querySelector('[data-testid="dialog-action-save"]') as HTMLButtonElement).disabled).toBe(false);
    expect(el.querySelector('[data-testid="diag-removed"]')).not.toBeNull();
  });

  it('styles every class its template uses (the Size column stays on one line, long cells break)', async () => {
    const { el } = await open();
    const used = new Set([...el.querySelectorAll('[class*="pd-diag__"]')].flatMap((n) => [...n.classList].filter((c) => c.startsWith('pd-diag__'))));
    expect(used.size).toBeGreaterThan(0);
    for (const c of used) expect(DIAGNOSTICS_STYLES, c).toContain(`.${c}`);
    expect(DIAGNOSTICS_STYLES).toMatch(/\.pd-diag__num[^}]*white-space: nowrap/);
    expect(DIAGNOSTICS_STYLES).toMatch(/\.pd-diag__what[^}]*overflow-wrap: anywhere/);
    expect(DIAGNOSTICS_STYLES).toMatch(/th\.pd-diag__name[^}]*overflow-wrap: anywhere[^}]*word-break: break-all[^}]*hyphens: none/);
  });

  it('follows the description when focus leaves the box, and saves it with the report', async () => {
    const { el, fixture, memory, closed, answer } = await open();
    const box = el.querySelector('textarea') as HTMLTextAreaElement;
    box.value = 'the timeline stays empty';
    box.dispatchEvent(new Event('input'));
    box.dispatchEvent(new Event('change'));
    await settle();
    await fixture.whenStable();
    expect(text(el.querySelector('[data-testid="diag-files"] tbody tr:nth-child(2)') as HTMLElement)).toContain('Your description (24 characters)');
    expect(memory.collections).toBe(1);
    (el.querySelector('[data-testid="dialog-action-save"]') as HTMLButtonElement).click();
    await fixture.whenStable();
    expect(text(el.querySelector('[data-testid="diag-saving"]') as HTMLElement)).toContain('Saving the report…');
    expect(box.disabled).toBe(true);
    await answer();
    expect(text(el.querySelector('[data-testid="diag-saved"]') as HTMLElement)).toContain('The report is saved.');
    expect(el.querySelector('[data-testid="diag-saved-path"]')?.getAttribute('title')).toContain('demo-diagnostics');
    expect(el.querySelector('[data-testid="dialog-action-save"]')).toBeNull();
    (el.querySelector('[data-testid="dialog-action-done"]') as HTMLButtonElement).click();
    await settle();
    expect(closed).toHaveLength(0);
    expect(fixture.componentInstance.answers).toHaveLength(1);
    expect(fixture.componentInstance.answers[0]).toMatchObject({ files: 4 });
  });

  it('shows a save that is stopped, closed or failed, and the next step', async () => {
    const dismissed = await open({ save: 'dismiss' });
    (dismissed.el.querySelector('[data-testid="dialog-action-save"]') as HTMLButtonElement).click();
    await dismissed.answer();
    expect(text(dismissed.el.querySelector('[data-testid="diag-dismissed"]') as HTMLElement)).toContain('Nothing was saved.');
    TestBed.resetTestingModule();
    const failed = await open({ save: 'fail' });
    (failed.el.querySelector('[data-testid="dialog-action-save"]') as HTMLButtonElement).click();
    await failed.answer();
    const alert = failed.el.querySelector('[data-testid="diag-save-error"]') as HTMLElement;
    expect(alert.getAttribute('role')).toBe('alert');
    expect(text(alert)).toContain('No space left on device. Choose another folder or free some space.');
    TestBed.resetTestingModule();
    const hanging = await open({ save: 'hang' });
    (hanging.el.querySelector('[data-testid="dialog-action-save"]') as HTMLButtonElement).click();
    await hanging.fixture.whenStable();
    button(hanging.el, 'Stop saving').click();
    await hanging.answer();
    expect(text(hanging.el.querySelector('[data-testid="diag-cancelled"]') as HTMLElement)).toContain('Nothing was written.');
    expect((hanging.el.querySelector('[data-testid="dialog-action-save"]') as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows the collection in progress, and an error with Retry that works', async () => {
    const memory = new MemoryDiagnosticsBackend({ files: FILES });
    let fail = true;
    vi.spyOn(memory, 'preview').mockImplementation(() => (fail ? Promise.reject(new Error('the engine is gone')) : MemoryDiagnosticsBackend.prototype.preview.call(memory, null, '')));
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideShellDiagnostics({ backend: () => memory }), { provide: DIALOG_REF, useValue: { answer: () => undefined, dismiss: () => undefined, closed: Promise.resolve(DISMISSED) } }],
    });
    const fixture = createFramed(PdReportProblemDialog, { kind: "form" });
    const el: HTMLElement = fixture.nativeElement;
    await fixture.whenStable();
    await settle();
    await fixture.whenStable();
    const alert = el.querySelector('[data-testid="diag-error"]') as HTMLElement;
    expect(alert.getAttribute('role')).toBe('alert');
    expect(text(alert)).toContain('the engine is gone');
    expect((el.querySelector('[data-testid="dialog-action-save"]') as HTMLButtonElement).disabled).toBe(true);
    fail = false;
    button(el, 'Retry').click();
    await settle();
    await fixture.whenStable();
    expect(el.querySelector('[data-testid="diag-error"]')).toBeNull();
    expect(el.querySelectorAll('[data-testid="diag-files"] tbody tr').length).toBe(4);
  });

  it('lists what could not be collected', async () => {
    const memory = new MemoryDiagnosticsBackend({ files: FILES });
    const reply = await memory.preview(null, '');
    vi.spyOn(memory, 'preview').mockResolvedValue({ ...reply, preview: { ...reply.preview, notes: ['There are no log files yet.'] } });
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideShellDiagnostics({ backend: () => memory }), { provide: DIALOG_REF, useValue: { answer: () => undefined, dismiss: () => undefined, closed: Promise.resolve(DISMISSED) } }],
    });
    const fixture = createFramed(PdReportProblemDialog, { kind: "form" });
    await fixture.whenStable();
    await settle();
    await fixture.whenStable();
    expect(text(fixture.nativeElement.querySelector('[data-testid="diag-notes"]'))).toBe('There are no log files yet.');
  });
});

describe('the contributions', () => {
  it('are valid, open the dialog from Help, and name only commands the dialog may call', async () => {
    const dialog = defineDiagnosticsDialog({ providers: () => Promise.resolve([]) });
    expect(dialog).toMatchObject({ id: DIAGNOSTICS_DIALOG_ID, kind: 'form', modality: 'modeless', width: 720 });
    expect(dialog.commands).toEqual([...DIAGNOSTICS_DIALOG_COMMANDS]);
    const batch = diagnosticsContributions([dialog]);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions(batch)] });
    assertContributionsValid(TestBed.inject(Injector));
    expect(batch.menus?.[0]).toMatchObject({ menu: 'help', command: 'pd.diagnostics.report' });
    expect(batch.settingsPages?.[0]).toMatchObject({ id: LOGGING_SETTINGS_PAGE, group: 'advanced' });
    expect(LOGGING_SETTINGS_PAGE).toBe('pd.logging');
    const opened: string[] = [];
    await batch.commands?.[0]?.run({ openDialog: (id: string) => (opened.push(id), Promise.resolve(undefined)) } as never);
    expect(opened).toEqual([DIAGNOSTICS_DIALOG_ID]);
  });
});

describe('the webview’s log', () => {
  function rig() {
    const sent: { level: string; message: string }[] = [];
    const ipc: WebviewLogIpc = { invoke: (_c, args) => (sent.push(args), Promise.resolve(null)) };
    return { sent, ipc, log: new WebviewLog(ipc) };
  }

  it('forwards a line with its level, cuts a long one, and never throws', () => {
    const { sent, log } = rig();
    log.forward('warn', 'slow render');
    log.forward('error', 'x'.repeat(MAX_FORWARDED_MESSAGE + 500));
    expect(sent[0]).toEqual({ level: 'warn', message: 'slow render' });
    expect([...(sent[1]?.message ?? '')]).toHaveLength(MAX_FORWARDED_MESSAGE + 1);
    const rejecting = new WebviewLog({ invoke: () => Promise.reject(new Error('no bridge')) });
    expect(() => rejecting.forward('error', 'x')).not.toThrow();
    const throwing = new WebviewLog({
      invoke: () => {
        throw new Error('no bridge');
      },
    });
    expect(() => throwing.forward('error', 'x')).not.toThrow();
  });

  it('a page that loops cannot flood the log: after the limit one line says so, then nothing', () => {
    const { sent, log } = rig();
    for (let i = 0; i < MAX_FORWARDED_LINES + 50; i++) log.forward('error', `boom ${i}`);
    expect(sent).toHaveLength(MAX_FORWARDED_LINES + 1);
    expect(sent.at(-1)).toMatchObject({ level: 'warn' });
    expect(sent.at(-1)?.message).toContain(`More than ${MAX_FORWARDED_LINES} lines`);
  });

  it('forwards uncaught errors and unhandled rejections until stopped', () => {
    const { sent, log } = rig();
    const target = new EventTarget();
    const stop = log.install(target as unknown as Window);
    const error = new Event('error') as ErrorEvent;
    Object.assign(error, { message: 'x is not defined', filename: 'app.js', lineno: 7 });
    target.dispatchEvent(error);
    for (const reason of [new Error('rejected'), 'plain', { code: 3 }, 12]) {
      const e = new Event('unhandledrejection');
      Object.assign(e, { reason });
      target.dispatchEvent(e);
    }
    const circular: Record<string, unknown> = {};
    circular['self'] = circular;
    const e = new Event('unhandledrejection');
    Object.assign(e, { reason: circular });
    target.dispatchEvent(e);
    expect(sent.map((s) => s.message)).toEqual([
      'Uncaught error: x is not defined (app.js:7)',
      'Unhandled promise rejection: rejected',
      'Unhandled promise rejection: plain',
      'Unhandled promise rejection: {"code":3}',
      'Unhandled promise rejection: 12',
      'Unhandled promise rejection: [object Object]',
    ]);
    stop();
    target.dispatchEvent(new Event('error'));
    expect(sent).toHaveLength(6);
  });

  it('Angular’s error handler logs through the engine and still reports to the console', () => {
    const { sent, ipc } = rig();
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideWebviewLog({ ipc: () => ipc })] });
    TestBed.inject(ErrorHandler).handleError(new Error('template failed'));
    expect(sent.some((s) => s.message === 'Unhandled error: template failed')).toBe(true);
    expect(consoleError).toHaveBeenCalled();
    consoleError.mockRestore();
  });
});
