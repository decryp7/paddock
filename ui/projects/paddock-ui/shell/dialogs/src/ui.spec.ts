import { ChangeDetectionStrategy, Component, computed, inject, Injectable, input, provideZonelessChangeDetection, signal, viewChild, type TemplateRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineDialog, DIALOG_EVENTS, dialogCommandLedger, msg, provideContributions, resetDialogCommands, SHELL_PORTS, type DialogDef } from '@decryp7/paddock-ui/shell';
import { fakeDialogContract, FakeDialogIo, renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { DialogActionError, type DialogAction } from './actions';
import { loadDialogBounds, rememberDialogBounds } from './bounds';
import { dialogWindowConfig, mountDialogWindowHost, openDialogPort, provideDialogs } from './bootstrap';
import { DeniedCommands } from './denied';
import { DIALOG_SHOW_DEADLINE_MS, FRAME_STYLES, oncePerFrame } from './dialog-frame';
import { DialogHost } from './dialog-host';
import { DialogShell } from './dialog-shell';
import { DialogService } from './dialog.service';
import { InPageDialogBackend, raise, type InPageDialog } from './in-page';
import { DIALOG_IO, tauriDialogIo } from './io';
import { ModalGate } from './modal-gate';
import { decodeOutcome, encodeOutcome } from './outcome';
import { DIALOG_ROUTE } from './route';
import { DIALOG_INPUT, DIALOG_LOOKUP, DIALOG_REF, DialogPresenter, type DialogContent } from './types';
import { parseClosed, parseResult, windowRequest, WindowDialogBackend } from './window-backend';

let allowClose = true;

@Injectable()
class Extra {
  readonly word = 'extra';
}

/** A content with a field and one primary action, OK, answering `'yes'`. */
@Component({
  selector: 'pd-test-content',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p data-testid="who">{{ who() }} {{ raw['n'] ?? '' }}</p>
    <input data-testid="field" />`,
})
class Content implements DialogContent<string> {
  readonly who = input('nobody');
  protected readonly raw = inject(DIALOG_INPUT);
  readonly dialogActions = signal<readonly DialogAction<string>[]>([{ kind: 'answer', id: 'ok', label: 'OK', role: 'primary', answer: () => 'yes' }]);
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

/** The actions the `Scripted` content declares (set per test). */
let scripted: readonly DialogAction<unknown>[] = [];

/** A content whose actions a test scripts. */
@Component({ selector: 'pd-test-scripted', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p data-testid="scripted">Pick one.</p><input data-testid="field" />` })
class Scripted implements DialogContent {
  readonly ref = inject(DIALOG_REF);
  readonly dialogActions = signal(scripted);
  readonly dialogStatus = signal<string | null>('3 things');
}

/** A content with a header that does not scroll (a stepper). */
@Component({ selector: 'pd-test-headed', changeDetection: ChangeDetectionStrategy.OnPush, template: `<ng-template #step><span data-testid="step">Step 1 of 3</span></ng-template><p>Body</p>` })
class Headed implements DialogContent {
  private readonly step = viewChild<TemplateRef<unknown>>('step');
  readonly dialogHeader = computed(() => this.step() ?? null);
  readonly dialogMinBodyHeight = 120;
}

type Spec = Partial<Pick<DialogDef, 'kind' | 'modality' | 'instanced' | 'requiredChoice' | 'result' | 'load' | 'providers'>>;
const d = (id: string, { kind = 'decision', modality, instanced, requiredChoice, ...ui }: Spec = {}): DialogDef =>
  defineDialog(fakeDialogContract({ id, kind, modality, instanced, requiredChoice }), { title: msg(id, `Title ${id}`), load: () => Promise.resolve(Content), ...ui }) as DialogDef;

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

/** A promise a test settles by hand. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => ((resolve = res), (reject = rej)));
  return { promise, resolve, reject };
}

beforeEach(() => {
  allowClose = true;
  scripted = [];
  localStorage.clear();
});

describe('the outcome on the wire', () => {
  it('tells an answer of undefined from a dismissal', () => {
    expect(encodeOutcome({ kind: 'dismissed' })).toBeNull();
    expect(encodeOutcome({ kind: 'answered', value: undefined })).toEqual({ answered: true });
    expect(encodeOutcome({ kind: 'answered', value: 0 })).toEqual({ answered: true, value: 0 });
    expect(decodeOutcome({ answered: true })).toEqual({ kind: 'answered', value: undefined });
    expect(decodeOutcome({ answered: true, value: null })).toEqual({ kind: 'answered', value: null });
    for (const x of [null, undefined, 'yes', { value: 1 }, { answered: 'true' }]) expect(decodeOutcome(x)).toEqual({ kind: 'dismissed' });
  });
});

describe('the dialog frame (ADR 0026 §2)', () => {
  const open = (actions: readonly DialogAction<unknown>[], options: Parameters<typeof renderFramed>[1] = {}) => {
    scripted = actions;
    return renderFramed(Scripted, options);
  };

  it('lays out the declared actions in the native order and draws one filled button', async () => {
    const f = await open([
      { kind: 'answer', id: 'save', label: 'Save', role: 'primary', answer: () => 's' },
      { kind: 'answer', id: 'dont-save', label: 'Don’t save', role: 'destructive', answer: () => 'd' },
    ]);
    const platform = f.el.querySelector('pd-dialog-frame')!.getAttribute('data-platform');
    expect(f.actions()).toEqual(platform === 'windows' ? ['save', 'dont-save', 'dismiss'] : ['dont-save', 'dismiss', 'save']);
    expect(f.el.querySelectorAll('.pd-frame-btn--filled')).toHaveLength(1);
    expect(f.q('dialog-status')!.textContent!.trim()).toBe('3 things');
    // In the page the dialog element is the alertdialog; the body takes no role of its own.
    expect(f.el.getAttribute('role')).toBe('alertdialog');
    expect(f.q('dialog-body')!.getAttribute('role')).toBeNull();
    expect(f.el.getAttribute('aria-describedby')).toBe(f.q('scripted')!.id);
  });

  it('renders the fallback Close for an invalid declaration and logs it', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const f = await open([
      { kind: 'answer', id: 'a', label: 'A', role: 'primary', answer: () => 1 },
      { kind: 'answer', id: 'b', label: 'B', role: 'primary', answer: () => 2 },
    ]);
    expect(f.actions()).toEqual(['dismiss']);
    expect(f.action('dismiss')!.textContent!.trim()).toBe('Close');
    expect(error.mock.calls.some((c) => String(c[0]).includes('2 primary actions'))).toBe(true);
    error.mockRestore();
  });

  it('is busy while an answer runs: every action disabled, a spinner and aria-busy, the body inert', async () => {
    const work = deferred<string>();
    const f = await open([{ kind: 'answer', id: 'go', label: 'Go', role: 'primary', answer: () => work.promise }]);
    await f.press('go');
    expect(f.action('go')!.getAttribute('aria-busy')).toBe('true');
    expect(f.action('go')!.querySelector('.pd-frame-spinner')).not.toBeNull();
    expect(f.action('go')!.disabled && f.action('dismiss')!.disabled).toBe(true);
    expect(f.q('dialog-body')!.hasAttribute('inert')).toBe(true);
    // The pressed button keeps its full colour (only the others dim) and the inert body is dimmed.
    expect(FRAME_STYLES).toContain(".pd-frame-btn[aria-busy='true']:disabled { opacity: 1;");
    expect(FRAME_STYLES).toContain('.pd-frame-body[inert] :is(input, select, textarea, button, label');
    expect(FRAME_STYLES).not.toContain('.pd-frame-body[inert] {');
    await f.key('Enter');
    work.resolve('done');
    await f.stable();
    expect(f.outcome()).toEqual({ kind: 'answered', value: 'done' });
  });

  it('says what it is doing on the busy button when the action names it (busyLabel)', async () => {
    const work = deferred<string>();
    const f = await open([{ kind: 'answer', id: 'go', label: 'Create key', busyLabel: 'Creating…', role: 'primary', answer: () => work.promise }]);
    expect(f.action('go')!.textContent!.trim()).toBe('Create key');
    await f.press('go');
    expect(f.action('go')!.textContent!.trim()).toBe('Creating…');
    work.resolve('done');
    await f.stable();
  });

  it('aborts an abortable answer on Escape and closes dismissed; a late answer is ignored', async () => {
    const work = deferred<string>();
    let aborted = false;
    const f = await open([
      {
        kind: 'answer',
        id: 'go',
        label: 'Go',
        role: 'primary',
        abortable: true,
        answer: (signal) => {
          signal.addEventListener('abort', () => (aborted = true));
          return work.promise;
        },
      },
    ]);
    await f.press('go');
    expect(f.action('dismiss')!.disabled).toBe(false);
    await f.key('Escape');
    expect(aborted).toBe(true);
    expect(f.outcome()).toEqual({ kind: 'dismissed' });
    work.resolve('late');
    await f.stable();
    expect(f.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('lets a non-abortable answer finish when dismissed meanwhile: "Finishing…", then its answer', async () => {
    const work = deferred<string>();
    const f = await open([{ kind: 'answer', id: 'go', label: 'Go', role: 'primary', answer: () => work.promise }]);
    await f.press('go');
    await f.key('Escape');
    expect(f.outcome()).toBeNull();
    expect(f.q('dialog-status')!.textContent!.trim()).toBe('Finishing…');
    work.resolve('saved');
    await f.stable();
    expect(f.outcome()).toEqual({ kind: 'answered', value: 'saved' });
  });

  it('applies a dismissal that waited when the work fails, and when a run ends', async () => {
    const fail = deferred<string>();
    const a = await open([{ kind: 'answer', id: 'go', label: 'Go', role: 'primary', answer: () => fail.promise }]);
    await a.press('go');
    await a.key('Escape');
    fail.reject(new Error('no'));
    await a.stable();
    expect(a.outcome()).toEqual({ kind: 'dismissed' });
    TestBed.resetTestingModule();
    const run = deferred<void>();
    const b = await open([{ kind: 'run', id: 'copy', label: 'Copy', role: 'secondary', run: () => run.promise }]);
    await b.press('copy');
    b.handle.dismiss();
    await b.stable();
    run.resolve();
    await b.stable();
    expect(b.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('keeps the dialog open on a rejection, with the user message, Try again (focused) and the input kept', async () => {
    let calls = 0;
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const f = await open([
      {
        kind: 'answer',
        id: 'save',
        label: 'Save',
        role: 'primary',
        answer: () => {
          calls++;
          if (calls === 1) throw new DialogActionError('The disk is full.');
          if (calls === 2) throw new Error('internal detail');
          return 'ok';
        },
      },
    ]);
    const field = f.q<HTMLInputElement>('field')!;
    field.value = 'typed';
    await f.press('save');
    expect(f.q('dialog-alert-text')!.textContent).toBe('The disk is full.');
    expect(document.activeElement).toBe(f.q('dialog-try-again'));
    // An error icon, no danger border, and Try again shows its focus ring although the focus was moved by script.
    expect(f.q('dialog-alert')!.querySelector('.pd-frame-alert-icon svg')).not.toBeNull();
    expect(FRAME_STYLES).toMatch(/\.pd-frame-alert \{[^}]*border: 0;/);
    expect(FRAME_STYLES).toContain('.pd-frame-alert .pd-frame-btn:focus {');
    expect(field.value).toBe('typed');
    f.q('dialog-try-again')!.click();
    await f.stable();
    expect(f.q('dialog-alert-text')!.textContent).toContain("That didn't work");
    expect(error.mock.calls.some((c) => String(c[0]).includes('action save of dialog t.framed failed'))).toBe(true);
    expect(error.mock.calls.flat().join(' ')).not.toContain('internal detail');
    f.q('dialog-try-again')!.click();
    await f.stable();
    expect(f.outcome()).toEqual({ kind: 'answered', value: 'ok' });
    error.mockRestore();
  });

  it('runs a run action without closing, skips a disabled one, and Enter never runs a destructive action', async () => {
    let ran = 0;
    const f = await open([
      { kind: 'run', id: 'copy', label: 'Copy', role: 'secondary', lead: true, run: () => void ran++ },
      { kind: 'answer', id: 'off', label: 'Off', role: 'primary', enabled: false, answer: () => 1 },
    ]);
    await f.press('copy');
    expect(ran).toBe(1);
    expect(f.outcome()).toBeNull();
    await f.press('off');
    await f.key('Enter');
    expect(f.outcome()).toBeNull();
    TestBed.resetTestingModule();
    const g = await open([{ kind: 'answer', id: 'delete', label: 'Delete 3 runners', role: 'destructive', answer: () => true }]);
    expect(document.activeElement).toBe(g.action('dismiss'));
    g.q('field')!.focus();
    await g.key('Enter');
    expect(g.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('focuses the heading of a required choice whose only answer destroys something', async () => {
    @Component({ selector: 'pd-test-wipe', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>Everything goes.</p>` })
    class Wipe implements DialogContent {
      readonly dialogHeading = signal('Wipe the workspace?');
      readonly dialogActions = signal<readonly DialogAction[]>([{ kind: 'answer', id: 'wipe', label: 'Wipe workspace', role: 'destructive', answer: () => true }]);
    }
    const f = await renderFramed(Wipe, { requiredChoice: true });
    expect(document.activeElement).toBe(f.q('dialog-heading'));
  });

  it('keeps a required choice open on every dismissal; the content may answer through its ref', async () => {
    const f = await open([{ kind: 'answer', id: 'formal', label: 'Formal', role: 'primary', answer: () => 'formal' }], { requiredChoice: true });
    expect(f.actions()).toEqual(['formal']);
    f.handle.dismiss();
    await f.key('Escape');
    expect(f.outcome()).toBeNull();
    TestBed.inject(InPageDialogBackend).stack()[0]!.ref.answer('picked');
    await f.stable();
    expect(f.outcome()).toEqual({ kind: 'answered', value: 'picked' });
  });

  it('draws the content’s header above the body, outside the scroller', async () => {
    const f = await renderFramed(Headed, { kind: 'form', modality: 'modeless' });
    expect(f.q('dialog-header')!.textContent).toContain('Step 1 of 3');
    expect(f.q('dialog-body')!.contains(f.q('dialog-header'))).toBe(false);
  });

  it('coalesces refits to one per frame, on the next task while the document is hidden', () => {
    vi.useFakeTimers();
    try {
      let runs = 0;
      const hidden = oncePerFrame(() => runs++, { visibilityState: 'hidden' });
      hidden.ask();
      hidden.ask();
      vi.advanceTimersByTime(0);
      expect(runs).toBe(1);
      hidden.ask();
      hidden.cancel();
      hidden.cancel();
      vi.advanceTimersByTime(10);
      expect(runs).toBe(1);
      const visible = oncePerFrame(() => runs++, { visibilityState: 'visible' });
      visible.ask();
      visible.ask();
      vi.advanceTimersByTime(40);
      expect(runs).toBe(2);
      visible.ask();
      visible.cancel();
      vi.advanceTimersByTime(40);
      expect(runs).toBe(2);
    } finally {
      vi.useRealTimers();
    }
  });
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
    expect(box.classList).not.toContain('pd-pending');
    expect(box.querySelector('h2')!.textContent).toBe('Title t.decision');
    expect(root.querySelector('[data-testid=who]')!.textContent).toBe('Ada 3');
    expect(root.querySelector('[data-testid=dialog-backdrop]')).not.toBeNull();
    expect(gate.blocked()).toBe(true);
    expect(document.activeElement).toBe(root.querySelector('[data-testid=dialog-action-ok]'));
    root.querySelector<HTMLElement>('[data-testid=dialog-action-ok]')!.click();
    expect(await ref.closed).toEqual({ kind: 'answered', value: 'yes' });
    await settle();
    expect(root.querySelector('[data-dialog]')).toBeNull();
    expect(gate.blocked()).toBe(false);
    expect(document.activeElement).toBe(before);
    before.remove();
  });

  it('stays hidden until it is ready, and shows Loading with its Cancel at the show deadline', async () => {
    const never = d('t.slow', { load: () => new Promise(() => undefined) });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions({ dialogs: [never] }), provideDialogs()] });
    const fixture = TestBed.createComponent(DialogHost);
    vi.useFakeTimers();
    try {
      const handle = TestBed.inject(DialogService).open(never);
      fixture.detectChanges();
      await vi.advanceTimersByTimeAsync(0);
      fixture.detectChanges();
      const root = fixture.nativeElement as HTMLElement;
      const box = root.querySelector<HTMLElement>('[data-dialog="t.slow"]')!;
      expect(box.classList).toContain('pd-pending');
      await vi.advanceTimersByTimeAsync(DIALOG_SHOW_DEADLINE_MS);
      fixture.detectChanges();
      expect(box.classList).not.toContain('pd-pending');
      expect(root.querySelector('[data-testid=dialog-loading]')).not.toBeNull();
      root.querySelector<HTMLElement>('[data-testid=dialog-action-dismiss]')!.click();
      expect(await handle.closed).toEqual({ kind: 'dismissed' });
    } finally {
      vi.useRealTimers();
    }
  });

  it('shows a title the content sets at run time in the header and keeps the declared one for a blank title', async () => {
    const { service, root, settle } = await setup();
    service.open(DECISION);
    await settle();
    const ref = TestBed.inject(InPageDialogBackend).stack()[0]!.ref;
    ref.setTitle?.('Delete 3 runners?');
    await settle();
    expect(root.querySelector('[data-dialog="t.decision"] h2')!.textContent).toBe('Delete 3 runners?');
    ref.setTitle?.('  ');
    ref.requestFit?.();
    await settle();
    expect(root.querySelector('[data-dialog="t.decision"] h2')!.textContent).toBe('Delete 3 runners?');
    expect(root.querySelector('[data-dialog="t.decision"]')!.getAttribute('aria-labelledby')).toBe(root.querySelector('[data-dialog="t.decision"] h2')!.id);
  });

  it('dismisses on Escape (inside and outside), the backdrop and the frame’s Cancel, unless the content refuses', async () => {
    const { service, root, settle } = await setup();
    let ref = service.open(DECISION);
    await settle();
    key('Escape', root.querySelector('[data-testid=field]')!);
    expect(await ref.closed).toEqual({ kind: 'dismissed' });
    ref = service.open(DECISION);
    await settle();
    key('Escape');
    expect(await ref.closed).toEqual({ kind: 'dismissed' });
    ref = service.open(DECISION);
    await settle();
    root.querySelector<HTMLElement>('[data-testid=dialog-backdrop]')!.dispatchEvent(new MouseEvent('mousedown'));
    expect(await ref.closed).toEqual({ kind: 'dismissed' });
    allowClose = false;
    ref = service.open(DECISION);
    await settle();
    root.querySelector<HTMLElement>('[data-testid=dialog-action-dismiss]')!.click();
    await settle();
    expect(root.querySelector('[data-dialog="t.decision"]')).not.toBeNull();
    allowClose = true;
    root.querySelector<HTMLElement>('[data-testid=dialog-action-dismiss]')!.click();
    expect(await ref.closed).toEqual({ kind: 'dismissed' });
    // Escape already handled by someone else, or with no modal open, does nothing.
    const e = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true });
    e.preventDefault();
    document.dispatchEvent(e);
    key('Escape');
  });

  it('keeps a required choice open on Escape and the backdrop, with no Cancel', async () => {
    const { service, root, settle } = await setup();
    const ref = service.open(CHOICE);
    await settle();
    expect(root.querySelector('[data-testid=dialog-action-dismiss]')).toBeNull();
    key('Escape');
    key('Escape', root.querySelector('[data-testid=field]')!);
    root.querySelector<HTMLElement>('[data-testid=dialog-backdrop]')!.dispatchEvent(new MouseEvent('mousedown'));
    ref.dismiss();
    await settle();
    expect(root.querySelector('[data-dialog="t.choice"]')).not.toBeNull();
    root.querySelector<HTMLElement>('[data-testid=dialog-action-ok]')!.click();
    expect(await ref.closed).toEqual({ kind: 'answered', value: 'yes' });
  });

  it('runs the Enter target from a field and traps the focus in a modal', async () => {
    const { service, root, settle } = await setup();
    const ref = service.open(DECISION);
    await settle();
    expect(root.querySelector('[data-dialog="t.decision"] .cdk-focus-trap-anchor')).not.toBeNull();
    key('Enter', root.querySelector('[data-testid=field]')!);
    expect(await ref.closed).toEqual({ kind: 'answered', value: 'yes' });
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
    // A modeless dialog answers Escape from inside only.
    key('Escape');
    expect(root.querySelector('[data-dialog="t.form"]')).not.toBeNull();
    key('Escape', root.querySelector<HTMLElement>('[data-dialog="t.form"] [data-testid=field]')!);
    expect(await a.closed).toEqual({ kind: 'dismissed' });
    b.dismiss();
    expect(await b.closed).toEqual({ kind: 'dismissed' });
  });

  it('stacks modal dialogs, gives a tool its height and shows a content that failed with Close', async () => {
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
    expect(root.querySelector('[data-dialog="t.broken"] [data-testid=dialog-error-reason]')!.textContent).toBe('content broke');
    expect(root.querySelector('[data-dialog="t.failing"] [data-testid=dialog-error-reason]')!.textContent).toBe('chunk missing');
    expect(root.querySelector('[data-dialog="t.broken"] [data-testid=dialog-action-dismiss]')!.textContent!.trim()).toBe('Close');
  });

  it('answers dismissed for an unknown dialog and routes the shell port by id', async () => {
    const { service, settle } = await setup();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect(await service.open('t.none').closed).toEqual({ kind: 'dismissed' });
    service.open('t.none').dismiss();
    warn.mockRestore();
    const port = TestBed.runInInjectionContext(() => openDialogPort());
    const answer = port('t.form', 'not a record');
    await settle();
    TestBed.inject(InPageDialogBackend).stack()[0]!.ref.answer(5);
    expect(await answer).toEqual({ kind: 'answered', value: 5 });
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

  it('dismisses an entry its view has not reached yet', () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const session = TestBed.inject(InPageDialogBackend).show({ def: DECISION, title: 'T', inputs: {} });
    session.focus();
    session.dismiss();
    expect(session.isOpen()).toBe(false);
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

  it('opens a window by kind, resolves with the outcome when it is gone, and remembers its place', async () => {
    const { io, service, gate } = setup();
    rememberDialogBounds('t.viewer', { dx: 5, dy: 6, width: 100, height: 100 });
    const ref = service.open(VIEWER, { inputs: { a: 1 }, title: 'Mine' });
    await tick();
    // The size is the Rust contract's: the request names none (ADR 0026 §3).
    expect(io.opened[0]).toEqual({ dialogId: 't.viewer', instance: undefined, title: 'Mine', input: { a: 1 }, remembered: { dx: 5, dy: 6, width: 680, height: 560 }, theme: expect.stringMatching(/^(light|dark)$/) });
    expect(gate.blocked()).toBe(false);
    io.emit(DIALOG_EVENTS.shown, 'garbage');
    io.emit(DIALOG_EVENTS.result, { nonsense: true });
    io.result('d-main-9', 't.viewer', 'other window');
    io.result('d-main-1', 't.viewer', { answered: true, value: { ok: true } });
    io.emit(DIALOG_EVENTS.closed, 'garbage');
    io.closed('d-main-1', 't.viewer', { dx: 1, dy: 2, width: 800, height: 600 });
    expect(await ref.closed).toEqual({ kind: 'answered', value: { ok: true } });
    expect(loadDialogBounds()['t.viewer']).toEqual({ dx: 1, dy: 2, width: 800, height: 600 });
  });

  it('blocks the window while a modal window is open, focuses an open one and forwards a dismissal as a request', async () => {
    const { io, service, gate } = setup();
    const ref = service.open(DECISION);
    await tick();
    // Not blocked until the window is on screen.
    expect(gate.blocked()).toBe(false);
    io.shown('d-main-1', 't.decision');
    await tick();
    expect(gate.blocked()).toBe(true);
    expect(service.open('t.decision')).toBe(ref);
    ref.dismiss();
    await tick();
    expect(io.opened).toHaveLength(2);
    expect(io.closeRequests).toEqual(['d-main-1']);
    io.result('d-main-1', 't.decision', null);
    io.closed('d-main-1', 't.decision');
    expect(await ref.closed).toEqual({ kind: 'dismissed' });
    expect(gate.blocked()).toBe(false);
  });

  it('never blocks the window for a modal that closed before it was shown', async () => {
    const { io, service, gate } = setup();
    const ref = service.open(DECISION);
    await tick();
    io.result('d-main-1', 't.decision', null);
    io.closed('d-main-1', 't.decision');
    expect(await ref.closed).toEqual({ kind: 'dismissed' });
    io.shown('d-main-1', 't.decision');
    await tick();
    expect(gate.blocked()).toBe(false);
  });

  it('does not block the window for an existing window that is still pending until its shown event comes', async () => {
    const { io, service, gate } = setup();
    // Another opener (a reloaded page) finds the window Rust opened for this dialog, which is not on screen yet.
    await io.open({ dialogId: 't.decision', title: 'x', input: {}, theme: 'light' });
    const ref = service.open(DECISION);
    await tick();
    expect(gate.blocked()).toBe(false);
    io.shown('d-main-1', 't.decision');
    await tick();
    expect(gate.blocked()).toBe(true);
    io.result('d-main-1', 't.decision', null);
    io.closed('d-main-1', 't.decision');
    await ref.closed;
    expect(gate.blocked()).toBe(false);
  });

  it('blocks at once for an existing window that is on screen already, and a pending one that closes never blocks', async () => {
    const { io, service, gate } = setup();
    await io.open({ dialogId: 't.decision', title: 'x', input: {}, theme: 'light' });
    io.shown('d-main-1', 't.decision');
    const ref = service.open(DECISION);
    await tick();
    expect(gate.blocked()).toBe(true);
    io.result('d-main-1', 't.decision', null);
    io.closed('d-main-1', 't.decision');
    await ref.closed;
    expect(gate.blocked()).toBe(false);
    // Pending, then closed before it ever showed.
    await io.open({ dialogId: 't.decision', title: 'x', input: {}, theme: 'light' });
    const again = service.open(DECISION);
    await tick();
    io.result('d-main-2', 't.decision', null);
    io.closed('d-main-2', 't.decision');
    await again.closed;
    await tick();
    expect(gate.blocked()).toBe(false);
  });

  it('forgets a window that was shown before its open reply when it closes, so a reused label does not show early', async () => {
    const { io, service, gate } = setup();
    const first = service.open(DECISION);
    await tick();
    // The events of a window overtake the reply to its `open` (no pending entry yet): shown, then closed.
    io.shown('d-main-2', 't.choice');
    io.closed('d-main-2', 't.choice');
    service.open(CHOICE);
    await tick();
    expect(io.opened).toHaveLength(2);
    expect(gate.blocked()).toBe(false);
    io.result('d-main-1', 't.decision', null);
    io.closed('d-main-1', 't.decision');
    await first.closed;
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
    page.stack()[0]!.ref.answer(3);
    expect(await ref.closed).toEqual({ kind: 'answered', value: 3 });
    const backend = TestBed.inject(WindowDialogBackend);
    const failing = Object.assign(Object.create(Object.getPrototypeOf(io)), io, { listen: () => Promise.reject(new Error('no events')) });
    (backend as unknown as { io: unknown }).io = failing;
    (backend as unknown as { listening: unknown }).listening = null;
    const s = backend.show({ def: TOOL, title: 't', inputs: {} });
    await tick();
    expect(page.stack().map((x) => x.def.id)).toEqual(['t.tool']);
    s.dismiss();
    expect((backend as unknown as { listening: unknown }).listening).toBeNull();
    warn.mockRestore();
  });

  it('builds the request and parses the events defensively', () => {
    document.documentElement.dataset['theme'] = 'light';
    expect(windowRequest({ def: DECISION, title: 'T', inputs: {} }, undefined)).toEqual({ dialogId: 't.decision', instance: undefined, title: 'T', input: {}, remembered: undefined, theme: 'light' });
    document.documentElement.dataset['theme'] = 'dark';
    expect(windowRequest({ def: DECISION, title: 'T', inputs: {} }, undefined).theme).toBe('dark');
    delete document.documentElement.dataset['theme'];
    expect(['light', 'dark']).toContain(windowRequest({ def: DECISION, title: 'T', inputs: {} }, undefined).theme);
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

  it('shows the content with its input, sends the ready handshake once with the measure, and answers', async () => {
    const { io, root, settle } = boot(DECISION);
    await settle();
    expect(root.querySelector('[data-testid=who]')!.textContent).toBe('Lin ');
    expect(document.title).toBe('Window t.decision');
    expect(io.readied).not.toBeNull();
    expect(io.measures).toHaveLength(1);
    expect(io.readied!.minContent.width).toBeGreaterThanOrEqual(320);
    expect(io.readied!.minContent.height).toBeLessThanOrEqual(io.readied!.content.height);
    // Method d: measured at the contract's width, whatever the (hidden) window's viewport was; released once shown.
    expect(io.readied!.content.width).toBe(DECISION.width);
    expect(io.readied!.viewportHeight).toBe(window.innerHeight);
    // Rust undoes the webview zoom with it (devicePixelRatio over the window's scale factor).
    expect(io.readied!.devicePixelRatio).toBe(window.devicePixelRatio);
    expect(root.querySelector<HTMLElement>('pd-dialog-frame')!.classList).not.toContain('pd-frame--pinned');
    root.querySelector<HTMLElement>('[data-testid=dialog-action-ok]')!.click();
    root.querySelector<HTMLElement>('[data-testid=dialog-action-ok]')!.click();
    await settle();
    expect(io.closedWith).toEqual([{ answered: true, value: 'yes' }]);
  });

  it('lays the frame out at the contract width while it measures, and reads again when a font loads meanwhile', async () => {
    // The fonts are ready at first; the content then starts loading one, which the handshake waits for.
    const late = deferred<unknown>();
    let reads = 0;
    const fake = {
      get status() {
        return reads > 1 ? 'loaded' : 'loading';
      },
      get ready() {
        reads++;
        return reads === 1 ? Promise.resolve() : late.promise;
      },
    };
    const had = Object.getOwnPropertyDescriptor(document, 'fonts');
    Object.defineProperty(document, 'fonts', { configurable: true, value: fake });
    try {
      const widths: string[] = [];
      const { io, root, settle } = boot(DECISION);
      const original = io.ready.bind(io);
      io.ready = (m) => {
        const el = root.querySelector<HTMLElement>('pd-dialog-frame')!;
        widths.push(el.classList.contains('pd-frame--pinned') ? el.style.getPropertyValue('--pd-frame-pinned-width') : 'unpinned');
        return original(m);
      };
      // The start is a pending task until the handshake: whenStable would wait for the font.
      await tick(5);
      expect(io.readied).toBeNull();
      late.resolve(undefined);
      await settle();
      expect(io.readied!.content.width).toBe(400);
      expect(widths).toEqual(['400px']);
      expect(reads).toBe(2);
    } finally {
      if (had) Object.defineProperty(document, 'fonts', had);
      else delete (document as { fonts?: unknown }).fonts;
    }
  });

  it('takes the title the content sets at run time: heading, page title and the OS title bar; blank titles are ignored', async () => {
    const { io, fixture, root, settle } = boot(DECISION);
    await settle();
    const ref = (fixture.componentInstance as unknown as { ref: { setTitle(t: string): void; requestFit(): void } }).ref;
    ref.setTitle('  Delete 3 runners?  ');
    ref.setTitle('   ');
    ref.requestFit();
    await settle();
    expect(document.title).toBe('Delete 3 runners?');
    expect(root.querySelector('main')!.getAttribute('aria-label')).toBe('Delete 3 runners?');
    expect(io.titles).toEqual(['Delete 3 runners?']);
  });

  it('starts even when the window system cannot listen for close requests', async () => {
    const io = new FakeDialogIo();
    io.listen = () => Promise.reject(new Error('no events'));
    io.start = { dialogId: 't.form', title: 'F', kind: 'form', modality: 'modeless', requiredChoice: false, input: {} };
    const config = dialogWindowConfig({ dialogs: ALL, route: { id: 't.form', host: 'main', label: 'd-main-1' } });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...config.providers, { provide: DIALOG_IO, useValue: io }] });
    const fixture = TestBed.createComponent(DialogShell);
    await tick();
    await fixture.whenStable();
    expect(io.readied).not.toBeNull();
  });

  it('keeps working when the window system cannot set a title', async () => {
    const { io, fixture, settle } = boot(DECISION);
    await settle();
    io.setTitle = () => Promise.reject(new Error('no permission'));
    (fixture.componentInstance as unknown as { ref: { setTitle(t: string): void } }).ref.setTitle('Kept');
    await settle();
    expect(document.title).toBe('Kept');
  });

  it('draws the tauri title through the bridge when it has one', async () => {
    const calls: string[] = [];
    const bridge = { invoke: () => Promise.resolve(undefined as never), listen: () => Promise.resolve(() => undefined), emitTo: () => Promise.resolve(), setWindowTitle: (t: string) => (calls.push(t), Promise.resolve()) };
    await tauriDialogIo(bridge).setTitle?.('Hello');
    expect(calls).toEqual(['Hello']);
    expect(tauriDialogIo({ ...bridge, setWindowTitle: undefined }).setTitle).toBeUndefined();
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
    expect(io.closedWith).toEqual([null]);
  });

  it('runs the Enter target on Enter in a field', async () => {
    const { io, root, settle } = boot(MODELESS);
    await settle();
    key('Enter', root.querySelector('[data-testid=field]')!);
    await settle();
    expect(io.closedWith).toEqual([{ answered: true, value: 'yes' }]);
  });

  it('dismisses on the close request event, and answers through its ref', async () => {
    const a = boot(MODELESS);
    await a.settle();
    a.io.closeRequested();
    await a.settle();
    expect(a.io.closedWith).toEqual([null]);
    TestBed.resetTestingModule();
    const b = boot(MODELESS);
    await b.settle();
    const ref = (b.fixture.componentInstance as unknown as { ref: { answer(v: unknown): void; dismiss(): void } }).ref;
    ref.answer(undefined);
    await b.settle();
    expect(b.io.closedWith).toEqual([{ answered: true }]);
    ref.dismiss();
  });

  it('adds Close when the content declares none and loads its providers; every kind sends ready once', async () => {
    const { io, root, settle } = boot(VIEWER);
    await settle();
    expect(root.querySelector('[data-testid=dialog-action-dismiss]')!.textContent!.trim()).toBe('Close');
    expect(io.fitted).toHaveLength(1);
    root.querySelector<HTMLElement>('[data-testid=dialog-action-dismiss]')!.click();
    await settle();
    expect(io.closedWith).toEqual([null]);
    TestBed.resetTestingModule();
    const tool = boot(TOOL);
    await tool.settle();
    expect(tool.io.readied).not.toBeNull();
  });

  it('keeps a required choice open and offers no Cancel', async () => {
    const { io, root, settle } = boot(CHOICE);
    await settle();
    key('Escape');
    io.closeRequested();
    await settle();
    expect(io.closedWith).toEqual([]);
    expect(root.querySelector('[data-testid=dialog-action-dismiss]')).toBeNull();
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
      expect(io.readied).not.toBeNull();
      const close = root.querySelector<HTMLElement>('[data-testid=dialog-action-dismiss]')!;
      expect(close.textContent!.trim()).toBe('Close');
      close.click();
      await settle();
      expect(io.closedWith).toEqual([null]);
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
    expect(io.closedWith).toEqual([null]);
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

  it('refits after the handshake when the content grows, once per change, and never before it', async () => {
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
      // A frame runs at once here: the refit is coalesced to one per frame.
      const raf = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((cb) => (queueMicrotask(() => cb(0)), 1));
      const { io, fixture, root, settle } = boot(VIEWER);
      await settle();
      expect(io.measures).toHaveLength(1);
      const content = root.querySelector('.pd-frame-content') as HTMLElement;
      const rect = vi.spyOn(content, 'getBoundingClientRect').mockReturnValue({ height: 900, width: 700 } as DOMRect);
      observers.forEach((o) => o());
      observers.forEach((o) => o());
      await settle();
      expect(io.measures).toHaveLength(2);
      expect(io.fitted.at(-1)).toBeGreaterThanOrEqual(900);
      // The same size again sends nothing.
      observers.forEach((o) => o());
      await settle();
      expect(io.measures).toHaveLength(2);
      rect.mockRestore();
      raf.mockRestore();
      fixture.destroy();
    } finally {
      (globalThis as { ResizeObserver?: unknown }).ResizeObserver = original;
    }
  });
});

describe('dialog bootstraps', () => {
  it('a dialog window has the route, the lookup and the app base set; a main window picks the presenter', () => {
    const config = dialogWindowConfig({ dialogs: [DECISION], providers: [{ provide: 'base', useValue: 1 }], route: { id: 't.decision', host: 'main', label: 'd-main-1' } });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...config.providers] });
    expect(TestBed.inject(DIALOG_ROUTE)?.id).toBe('t.decision');
    expect(TestBed.inject(DIALOG_LOOKUP)('t.decision')).toBe(DECISION);
    expect(TestBed.inject('base' as never)).toBe(1);
    // The window's commands are recorded for its dialog, against its contract (ADR 0026 §3).
    expect([dialogCommandLedger().active, dialogCommandLedger().allowed?.includes('dialog_close')]).toEqual(['t.decision', true]);
    resetDialogCommands();
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
    // Not before the window is on screen.
    expect((fixture.nativeElement as HTMLElement).querySelector('[data-testid=dialog-scrim]')).toBeNull();
    io.shown('d-main-1', 't.decision');
    await tick();
    await fixture.whenStable();
    expect((fixture.nativeElement as HTMLElement).querySelector('[data-testid=dialog-scrim]')).not.toBeNull();
    io.closed('d-main-1', 't.decision');
    await ref.closed;
    await tick();
    await fixture.whenStable();
    expect((fixture.nativeElement as HTMLElement).querySelector('[data-testid=dialog-scrim]')).toBeNull();
  });
});
