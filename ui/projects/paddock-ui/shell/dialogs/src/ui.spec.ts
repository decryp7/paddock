import { ChangeDetectionStrategy, Component, inject, Injectable, input, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineDialog, DIALOG_EVENTS, msg, provideContributions, SHELL_PORTS, type DialogDef } from '@decryp7/paddock-ui/shell';
import { FakeDialogIo } from '@decryp7/paddock-ui/shell/testing';
import { loadDialogBounds, rememberDialogBounds } from './bounds';
import { dialogWindowConfig, mountDialogWindowHost, openDialogPort, provideDialogs } from './bootstrap';
import { DeniedCommands } from './denied';
import { DialogHost } from './dialog-host';
import { DialogShell } from './dialog-shell';
import { DialogService } from './dialog.service';
import { InPageDialogBackend, raise, type InPageDialog } from './in-page';
import { DIALOG_IO } from './io';
import { ModalGate } from './modal-gate';
import { DIALOG_ROUTE } from './route';
import { DIALOG_INPUT, DIALOG_LOOKUP, DIALOG_REF, DialogPresenter } from './types';
import { parseClosed, parseResult, windowRequest, WindowDialogBackend } from './window-backend';

let allowClose = true;

@Injectable()
class Extra {
  readonly word = 'extra';
}

@Component({
  selector: 'pd-test-content',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p data-testid="who">{{ who() }} {{ raw['n'] ?? '' }}</p>
    <input data-testid="field" />
    <button type="button" data-testid="cancel" (click)="ref.close()">Cancel</button>
    <button type="button" class="primary" data-testid="ok" (click)="ref.close('yes')">OK</button>`,
})
class Content {
  readonly who = input('nobody');
  protected readonly ref = inject(DIALOG_REF);
  protected readonly raw = inject(DIALOG_INPUT);
  dialogCanClose(): boolean {
    return allowClose;
  }
}

@Component({ selector: 'pd-test-plain', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>Just text</p>` })
class Plain {
  protected readonly extra = inject(Extra);
}

@Component({ selector: 'pd-test-throws', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>x</p>` })
class Throws {
  constructor() {
    throw new Error('content broke');
  }
}

const d = (id: string, spec: Partial<DialogDef> = {}): DialogDef =>
  defineDialog({ id, kind: 'decision', title: msg(id, `Title ${id}`), commands: [], load: () => Promise.resolve(Content), ...spec }) as DialogDef;

const DECISION = d('t.decision', { result: undefined });
const MODELESS = d('t.form', { kind: 'form', modality: 'modeless' });
const VIEWER = d('t.viewer', { kind: 'viewer', load: () => Promise.resolve(Plain), providers: () => Promise.resolve([Extra]) });
const TOOL = d('t.tool', { kind: 'tool' });
const CHOICE = d('t.choice', { requiredChoice: true });
const INSTANCED = d('t.inst', { kind: 'viewer', instanced: true });
const BROKEN = d('t.broken', { load: () => Promise.resolve(Throws) });
const FAILING = d('t.failing', { kind: 'form', modality: 'modeless', load: () => Promise.reject(new Error('chunk missing')) });
const ALL = [DECISION, MODELESS, VIEWER, TOOL, CHOICE, INSTANCED, BROKEN, FAILING];

const tick = async (n = 3) => {
  for (let i = 0; i < n; i++) {
    await new Promise<void>((r) => setTimeout(r, 0));
  }
};

function key(k: string, target: EventTarget = document, extra: KeyboardEventInit = {}): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...extra });
  target.dispatchEvent(e);
  return e;
}

beforeEach(() => {
  allowClose = true;
  localStorage.clear();
});

describe('dialogs in the page (browser build)', () => {
  async function setup() {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions({ dialogs: ALL }), provideDialogs()] });
    const fixture = TestBed.createComponent(DialogHost);
    const service = TestBed.inject(DialogService);
    const root = fixture.nativeElement as HTMLElement;
    const settle = async () => {
      await tick();
      await fixture.whenStable();
    };
    return { fixture, service, root, settle, gate: TestBed.inject(ModalGate) };
  }

  it('opens a modal over a backdrop with its inputs, answers, blocks the window meanwhile and gives the focus back', async () => {
    const { service, root, settle, gate } = await setup();
    const before = document.createElement('button');
    document.body.append(before);
    before.focus();
    const ref = service.open(DECISION, { inputs: { who: 'Ada', n: 3, unknown: true } });
    await settle();
    const box = root.querySelector<HTMLElement>('[data-dialog="t.decision"]')!;
    expect(box.getAttribute('aria-modal')).toBe('true');
    expect(box.querySelector('h2')!.textContent).toBe('Title t.decision');
    expect(root.querySelector('[data-testid=who]')!.textContent).toBe('Ada 3');
    expect(root.querySelector('[data-testid=dialog-backdrop]')).not.toBeNull();
    expect(gate.blocked()).toBe(true);
    expect(document.activeElement).toBe(root.querySelector('[data-testid=field]'));
    root.querySelector<HTMLElement>('[data-testid=ok]')!.click();
    expect(await ref.closed).toBe('yes');
    await settle();
    expect(root.querySelector('[data-dialog]')).toBeNull();
    expect(gate.blocked()).toBe(false);
    expect(document.activeElement).toBe(before);
    before.remove();
  });

  it('dismisses on Escape (inside and outside), the backdrop and the title-bar Close, unless the content refuses', async () => {
    const { service, root, settle } = await setup();
    let ref = service.open(DECISION);
    await settle();
    key('Escape', root.querySelector('[data-testid=field]')!);
    expect(await ref.closed).toBeUndefined();
    ref = service.open(DECISION);
    await settle();
    key('Escape');
    expect(await ref.closed).toBeUndefined();
    ref = service.open(DECISION);
    await settle();
    root.querySelector<HTMLElement>('[data-testid=dialog-backdrop]')!.dispatchEvent(new MouseEvent('mousedown'));
    expect(await ref.closed).toBeUndefined();
    allowClose = false;
    ref = service.open(DECISION);
    await settle();
    root.querySelector<HTMLElement>('[data-testid=dialog-close]')!.click();
    await settle();
    expect(root.querySelector('[data-dialog="t.decision"]')).not.toBeNull();
    allowClose = true;
    root.querySelector<HTMLElement>('[data-testid=dialog-close]')!.click();
    expect(await ref.closed).toBeUndefined();
    // Escape already handled by someone else, or with no modal open, does nothing.
    const e = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true });
    e.preventDefault();
    document.dispatchEvent(e);
    key('Escape');
  });

  it('keeps a required choice open on Escape and the backdrop, with no Close', async () => {
    const { service, root, settle } = await setup();
    const ref = service.open(CHOICE);
    await settle();
    expect(root.querySelector('[data-testid=dialog-close]')).toBeNull();
    key('Escape');
    key('Escape', root.querySelector('[data-testid=field]')!);
    root.querySelector<HTMLElement>('[data-testid=dialog-backdrop]')!.dispatchEvent(new MouseEvent('mousedown'));
    await settle();
    expect(root.querySelector('[data-dialog="t.choice"]')).not.toBeNull();
    ref.close('picked');
    expect(await ref.closed).toBe('picked');
  });

  it('runs the default action on Enter and traps Tab in a modal', async () => {
    const { service, root, settle } = await setup();
    const ref = service.open(DECISION);
    await settle();
    const field = root.querySelector<HTMLElement>('[data-testid=field]')!;
    const close = root.querySelector<HTMLElement>('[data-testid=dialog-close]')!;
    const ok = root.querySelector<HTMLElement>('[data-testid=ok]')!;
    ok.focus();
    expect(key('Tab', ok).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(close);
    expect(key('Tab', close, { shiftKey: true }).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(ok);
    field.focus();
    expect(key('Tab', field).defaultPrevented).toBe(false);
    key('Enter', field);
    expect(await ref.closed).toBe('yes');
  });

  it('opens a modeless dialog once without a backdrop, and focusing it raises it', async () => {
    const { service, root, settle, gate } = await setup();
    const a = service.open(MODELESS);
    const again = service.open('t.form');
    expect(again).toBe(a);
    const b = service.open(VIEWER);
    await settle();
    expect(root.querySelector('[data-testid=dialog-backdrop]')).toBeNull();
    expect(gate.blocked()).toBe(false);
    expect([...root.querySelectorAll('[data-dialog]')].map((e) => e.getAttribute('data-dialog'))).toEqual(['t.form', 't.viewer']);
    service.open(MODELESS);
    await settle();
    expect([...root.querySelectorAll('[data-dialog]')].map((e) => e.getAttribute('data-dialog'))).toEqual(['t.viewer', 't.form']);
    // A modeless dialog answers Escape from inside only, and Tab is not trapped.
    key('Escape');
    const field = root.querySelector<HTMLElement>('[data-dialog="t.form"] [data-testid=field]')!;
    expect(key('Tab', root.querySelector<HTMLElement>('[data-dialog="t.form"] [data-testid=ok]')!).defaultPrevented).toBe(false);
    key('Enter', root.querySelector('[data-dialog="t.viewer"]')!);
    key('Escape', field);
    expect(await a.closed).toBeUndefined();
    b.close();
    expect(await b.closed).toBeUndefined();
    // The viewer loaded its own providers.
  });

  it('stacks modal dialogs, gives a tool its height and shows a content that failed', async () => {
    const { service, root, settle } = await setup();
    const one = service.open(DECISION);
    const two = service.open(DECISION);
    expect(two).not.toBe(one);
    service.open(TOOL);
    service.open(BROKEN);
    service.open(FAILING);
    const spy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    await settle();
    spy.mockRestore();
    expect(root.querySelectorAll('[data-dialog="t.decision"]')).toHaveLength(2);
    expect(root.querySelector<HTMLElement>('[data-dialog="t.tool"]')!.style.height).toBe('680px');
    expect(root.querySelector('[data-dialog="t.broken"] [data-testid=dialog-error]')!.textContent).toBe('content broke');
    expect(root.querySelector('[data-dialog="t.failing"] [data-testid=dialog-error]')!.textContent).toBe('chunk missing');
  });

  it('answers undefined for an unknown dialog and routes the shell port by id', async () => {
    const { service, settle } = await setup();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect(await service.open('t.none').closed).toBeUndefined();
    service.open('t.none').close();
    warn.mockRestore();
    const port = TestBed.runInInjectionContext(() => openDialogPort());
    const answer = port('t.form', 'not a record');
    await settle();
    TestBed.inject(InPageDialogBackend).stack()[0]!.ref.close(5);
    expect(await answer).toBe(5);
    expect(service.openById('t.form', [1])).toBeInstanceOf(Promise);
  });

  it('raises a dialog only when no modal was opened after it', () => {
    const e = (n: number, modality: 'modal' | 'modeless') => ({ n, def: { modality } }) as unknown as InPageDialog;
    const s = [e(1, 'modeless'), e(2, 'modal')];
    expect(raise(s, 1)).toBe(s);
    expect(raise(s, 9)).toBe(s);
    expect(raise(s, 2)).toBe(s);
    const t = [e(1, 'modeless'), e(2, 'modeless')];
    expect(raise(t, 1).map((x) => x.n)).toEqual([2, 1]);
  });
});

describe('dialogs as windows', () => {
  function setup(extra: unknown[] = []) {
    const io = new FakeDialogIo();
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideContributions({ dialogs: ALL }), provideDialogs({ io: () => io }), ...(extra as never[])],
    });
    return { io, service: TestBed.inject(DialogService), gate: TestBed.inject(ModalGate) };
  }

  it('opens a window by kind, resolves with the answer when it is gone, and remembers its place', async () => {
    const { io, service, gate } = setup();
    rememberDialogBounds('t.viewer', { dx: 5, dy: 6, width: 100, height: 100 });
    const ref = service.open(VIEWER, { inputs: { a: 1 }, title: 'Mine' });
    await tick();
    expect(io.opened[0]).toEqual({ dialogId: 't.viewer', instance: undefined, title: 'Mine', input: { a: 1 }, width: 680, height: 560, minWidth: 680, minHeight: 560, remembered: { dx: 5, dy: 6, width: 680, height: 560 } });
    expect(gate.blocked()).toBe(false);
    io.emit(DIALOG_EVENTS.result, { nonsense: true });
    io.result('d-main-9', 't.viewer', 'other window');
    io.result('d-main-1', 't.viewer', { ok: true });
    io.emit(DIALOG_EVENTS.closed, 'garbage');
    io.closed('d-main-1', 't.viewer', { dx: 1, dy: 2, width: 800, height: 600 });
    expect(await ref.closed).toEqual({ ok: true });
    expect(loadDialogBounds()['t.viewer']).toEqual({ dx: 1, dy: 2, width: 800, height: 600 });
  });

  it('blocks the window while a modal window is open, focuses an open one and forwards close as a request', async () => {
    const { io, service, gate } = setup();
    const ref = service.open(DECISION);
    expect(gate.blocked()).toBe(true);
    await tick();
    expect(service.open('t.decision')).toBe(ref);
    ref.close('x');
    await tick();
    expect(io.opened).toHaveLength(2);
    expect(io.closeRequests).toEqual(['d-main-1']);
    io.result('d-main-1', 't.decision', null);
    io.closed('d-main-1', 't.decision');
    expect(await ref.closed).toBeUndefined();
    expect(gate.blocked()).toBe(false);
  });

  it('opens one window per input of an instanced dialog', async () => {
    const { io, service } = setup();
    service.open(INSTANCED, { inputs: { id: 1 } });
    service.open(INSTANCED, { inputs: { id: 2 } });
    await tick();
    expect(io.opened.map((r) => r.instance)).toHaveLength(2);
    expect(io.opened[0]!.instance).not.toBe(io.opened[1]!.instance);
  });

  it('shows the dialog in the page when its window cannot open, and retries listening later', async () => {
    const { io, service } = setup();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    io.failOpen = new Error('no window system');
    const ref = service.open(MODELESS);
    await tick();
    const page = TestBed.inject(InPageDialogBackend);
    expect(page.stack().map((x) => x.def.id)).toEqual(['t.form']);
    service.open(MODELESS);
    await tick();
    ref.close(3);
    expect(await ref.closed).toBe(3);
    const backend = TestBed.inject(WindowDialogBackend);
    const failing = Object.assign(Object.create(Object.getPrototypeOf(io)), io, { listen: () => Promise.reject(new Error('no events')) });
    (backend as unknown as { io: unknown }).io = failing;
    (backend as unknown as { listening: unknown }).listening = null;
    const s = backend.show({ def: TOOL, title: 't', inputs: {} });
    await tick();
    expect(page.stack().map((x) => x.def.id)).toEqual(['t.tool']);
    s.close();
    expect((backend as unknown as { listening: unknown }).listening).toBeNull();
    warn.mockRestore();
  });

  it('builds the request and parses the events defensively', () => {
    expect(windowRequest({ def: DECISION, title: 'T', inputs: {} }, undefined)).toMatchObject({ width: 640, height: undefined, remembered: undefined });
    expect(parseResult(null)).toBeNull();
    expect(parseClosed({ label: 'a', dialogId: 'b', remembered: { dx: 'x' } })).toEqual({ label: 'a', dialogId: 'b', remembered: null });
    expect(parseClosed({ label: 1 })).toBeNull();
  });
});

describe('a dialog window', () => {
  function boot(def: DialogDef, start: Partial<FakeDialogIo['start']> = {}, route = { id: def.id, host: 'main', label: 'd-main-1' }) {
    const io = new FakeDialogIo();
    io.start = { dialogId: def.id, title: `Window ${def.id}`, kind: def.kind, modality: def.modality, requiredChoice: false, input: { who: 'Lin' }, ...start };
    const config = dialogWindowConfig({ dialogs: ALL, route });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...config.providers, { provide: DIALOG_IO, useValue: io }] });
    const fixture = TestBed.createComponent(DialogShell);
    const root = fixture.nativeElement as HTMLElement;
    const settle = async (n = 3) => {
      for (let i = 0; i < n; i++) {
        await new Promise<void>((r) => setTimeout(r, 0));
        await fixture.whenStable();
      }
    };
    return { io, fixture, root, settle };
  }

  it('shows the content with its input, answers through the opener and fits a decision to its content', async () => {
    const { io, root, settle } = boot(DECISION);
    await settle();
    expect(root.querySelector('[data-testid=who]')!.textContent).toBe('Lin ');
    expect(document.title).toBe('Window t.decision');
    expect(root.querySelector('[data-testid=dialog-shell-foot]')).toBeNull();
    await settle(25);
    expect(io.fitted.length).toBeGreaterThan(0);
    root.querySelector<HTMLElement>('[data-testid=ok]')!.click();
    root.querySelector<HTMLElement>('[data-testid=ok]')!.click();
    await settle();
    expect(io.closedWith).toEqual(['yes']);
  });

  it('dismisses on Escape and Cmd+W unless the content refuses', async () => {
    const { io, settle } = boot(MODELESS);
    await settle();
    allowClose = false;
    key('Escape');
    await settle();
    expect(io.closedWith).toEqual([]);
    allowClose = true;
    key('w', document, { metaKey: true });
    await settle();
    expect(io.closedWith).toEqual([undefined]);
  });

  it('runs the default action on Enter', async () => {
    const { io, root, settle } = boot(MODELESS);
    await settle();
    key('Enter', root.querySelector('[data-testid=field]')!);
    await settle();
    expect(io.closedWith).toEqual(['yes']);
  });

  it('closes on the close request event', async () => {
    const { io, settle } = boot(MODELESS);
    await settle();
    io.closeRequested();
    await settle();
    expect(io.closedWith).toEqual([undefined]);
  });

  it('adds Close when the content has none, loads its providers and fits a viewer; a tool keeps its size', async () => {
    const { io, root, settle } = boot(VIEWER);
    await settle(25);
    expect(root.querySelector('[data-testid=dialog-shell-close]')!.textContent!.trim()).toBe('Close');
    expect(io.fitted.length).toBeGreaterThan(0);
    root.querySelector<HTMLElement>('[data-testid=dialog-shell-close]')!.click();
    await settle();
    expect(io.closedWith).toEqual([undefined]);
    TestBed.resetTestingModule();
    const tool = boot(TOOL);
    await tool.settle(25);
    expect(tool.io.fitted).toEqual([]);
  });

  it('keeps a required choice open and offers no Cancel', async () => {
    const { io, root, settle } = boot(CHOICE);
    await settle();
    key('Escape');
    io.closeRequested();
    await settle();
    expect(io.closedWith).toEqual([]);
    expect(root.querySelector('[data-testid=dialog-shell-close]')).toBeNull();
  });

  it('shows why it could not open, with a Close, for an unknown dialog, a failing load and a throwing content', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    for (const [def, reason] of [
      [d('t.unknown'), /not known to this version/],
      [FAILING, /chunk missing/],
      [BROKEN, /content broke/],
    ] as const) {
      const { io, root, settle } = boot(def as DialogDef);
      await settle(5);
      expect(root.querySelector('[data-testid=dialog-error-reason]')!.textContent).toMatch(reason);
      const close = root.querySelector<HTMLElement>('[data-testid=dialog-shell-close]')!;
      expect(close.textContent!.trim()).toBe('Close');
      close.click();
      await settle();
      expect(io.closedWith).toEqual([undefined]);
      TestBed.resetTestingModule();
    }
    spy.mockRestore();
  });

  it('a required choice that failed can still be closed; a route without an id is unknown', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const failingChoice = d('t.choice', { requiredChoice: true, load: () => Promise.reject(new Error('gone')) });
    const io = new FakeDialogIo();
    const config = dialogWindowConfig({ dialogs: [failingChoice], route: { id: 't.choice', host: 'main', label: 'd-main-1' } });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...config.providers, { provide: DIALOG_IO, useValue: io }] });
    const fixture = TestBed.createComponent(DialogShell);
    for (let i = 0; i < 5; i++) {
      await new Promise<void>((r) => setTimeout(r, 0));
      await fixture.whenStable();
    }
    key('Escape');
    await fixture.whenStable();
    await new Promise<void>((r) => setTimeout(r, 0));
    expect(io.closedWith).toEqual([undefined]);
    TestBed.resetTestingModule();
    const none = boot(DECISION, {}, null as never);
    await none.settle(5);
    expect(none.root.querySelector('[data-testid=dialog-error-reason]')!.textContent).toMatch(/\(none\)/);
    spy.mockRestore();
  });

  it('shows a refused command and survives a close that fails', async () => {
    const { io, root, settle } = boot(MODELESS);
    await settle();
    const spy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    TestBed.inject(DeniedCommands).report({ code: 'denied', command: 'x', message: 'x is not available in this dialog', retryable: false });
    await settle();
    expect(root.querySelector('[data-testid=dialog-denied]')!.textContent).toBe('x is not available in this dialog');
    io.close = () => Promise.reject(new Error('gone'));
    key('Escape');
    await settle();
    expect(spy.mock.calls.some((c) => String(c[0]).includes('could not close'))).toBe(true);
    spy.mockRestore();
  });
});

describe('dialog bootstraps', () => {
  it('a dialog window has the route, the lookup and the app base set; a main window picks the presenter', () => {
    const config = dialogWindowConfig({ dialogs: [DECISION], providers: [{ provide: 'base', useValue: 1 }], route: { id: 't.decision', host: 'main', label: 'd-main-1' } });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...config.providers] });
    expect(TestBed.inject(DIALOG_ROUTE)?.id).toBe('t.decision');
    expect(TestBed.inject(DIALOG_LOOKUP)('t.decision')).toBe(DECISION);
    expect(TestBed.inject('base' as never)).toBe(1);
    expect(TestBed.inject(DialogPresenter).windowed).toBe(false);
    TestBed.resetTestingModule();
    location.hash = '#/dialog?id=t.decision&host=main&label=d-main-2';
    const fromLocation = dialogWindowConfig({ dialogs: [] });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...fromLocation.providers] });
    expect(TestBed.inject(DIALOG_ROUTE)?.label).toBe('d-main-2');
    location.hash = '';
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideDialogs({ io: () => new FakeDialogIo() })] });
    expect(TestBed.inject(DialogPresenter).windowed).toBe(true);
    expect(TestBed.inject(SHELL_PORTS)).toBeTruthy();
  });

  it('defaults to the in-page presenter and the registry lookup without provideDialogs', () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions({ dialogs: [DECISION] })] });
    expect(TestBed.inject(DIALOG_LOOKUP)('t.decision')).toBe(DECISION);
    expect(TestBed.inject(DIALOG_ROUTE)).toBeNull();
    expect(TestBed.inject(DIALOG_IO)).toBeTruthy();
    expect(TestBed.inject(DialogService)).toBeTruthy();
  });

  it('mounts the dialog shell element in place of the page', () => {
    const doc = document.implementation.createHTMLDocument('x');
    doc.body.innerHTML = '<app-root>splash</app-root>';
    expect(mountDialogWindowHost(doc).tagName).toBe('PD-DIALOG-SHELL');
    expect(doc.body.children).toHaveLength(1);
  });
});

describe('window layout details', () => {
  it('draws a scrim over the window while a modal dialog window is open', async () => {
    const io = new FakeDialogIo();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions({ dialogs: ALL }), provideDialogs({ io: () => io })] });
    const fixture = TestBed.createComponent(DialogHost);
    const ref = TestBed.inject(DialogService).open(DECISION);
    await tick();
    await fixture.whenStable();
    expect((fixture.nativeElement as HTMLElement).querySelector('[data-testid=dialog-scrim]')).not.toBeNull();
    io.closed('d-main-1', 't.decision');
    await ref.closed;
    await tick();
    await fixture.whenStable();
    expect((fixture.nativeElement as HTMLElement).querySelector('[data-testid=dialog-scrim]')).toBeNull();
  });

  it('fits the window again when its content grows', async () => {
    const observers: (() => void)[] = [];
    const original = (globalThis as { ResizeObserver?: unknown }).ResizeObserver;
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = class {
      constructor(private readonly cb: () => void) {
        observers.push(() => this.cb());
      }
      observe(): void {}
      disconnect(): void {}
    };
    try {
      const io = new FakeDialogIo();
      io.start = { dialogId: 't.viewer', title: 'V', kind: 'viewer', modality: 'modeless', requiredChoice: false, input: {} };
      const config = dialogWindowConfig({ dialogs: ALL, route: { id: 't.viewer', host: 'main', label: 'd-main-1' } });
      TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...config.providers, { provide: DIALOG_IO, useValue: io }] });
      const fixture = TestBed.createComponent(DialogShell);
      const settle = async (n: number) => {
        for (let i = 0; i < n; i++) {
          await new Promise<void>((r) => setTimeout(r, 0));
          await fixture.whenStable();
        }
      };
      await settle(25);
      const first = io.fitted.length;
      expect(first).toBeGreaterThan(0);
      const body = (fixture.nativeElement as HTMLElement).querySelector('main')!;
      const rect = vi.spyOn(body.lastElementChild as HTMLElement, 'getBoundingClientRect').mockReturnValue({ bottom: 900 } as DOMRect);
      observers.forEach((o) => o());
      // The re-fit runs on the next frame.
      await new Promise<void>((r) => (typeof requestAnimationFrame === 'function' ? requestAnimationFrame(() => r()) : setTimeout(r, 0)));
      await settle(1);
      expect(io.fitted.at(-1)).toBeGreaterThan(900);
      rect.mockRestore();
      fixture.destroy();
    } finally {
      (globalThis as { ResizeObserver?: unknown }).ResizeObserver = original;
    }
  });
});
