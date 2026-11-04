import {
  type AfterViewInit,
  ApplicationRef,
  ChangeDetectionStrategy,
  ChangeDetectorRef,
  Component,
  DestroyRef,
  EnvironmentInjector,
  inject,
  Injector,
  PendingTasks,
  signal,
  viewChild,
  ViewContainerRef,
} from '@angular/core';
import { answered, DIALOG_EVENTS, DISMISSED, loadPart, type DialogDef, type DialogModality, type DialogOutcome } from '@decryp7/paddock-ui/shell';
import { DeniedCommands } from './denied';
import { DialogFrame, type FrameState } from './dialog-frame';
import { DIALOG_IO } from './io';
import { contentInjector, errorText, isRecord, mountContent } from './mount';
import { followDialogInputs } from './retarget';
import { encodeOutcome } from './outcome';
import { DIALOG_ROUTE } from './route';
import { SizeLoop } from './size-loop';
import { DIALOG_LOOKUP, type DialogContent, type DialogRef } from './types';

/**
 * The small app a dialog window boots into (ADR 0009, rule 34; ADR 0026 §2 and §4): the `DialogFrame` with the dialog's
 * content under the OS title bar, which has no buttons. It takes its input from Rust, loads the dialog's component with
 * only its declared providers, and sends the ready handshake once the content and the fonts are there, measured without
 * waiting for a frame (a hidden window may never run one). The frame owns the footer, the keys and the busy and rejected
 * states; the shell closes the window with the outcome. Every state has a way out: Loading and a failure have Close.
 */
@Component({
  selector: 'pd-dialog-shell',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DialogFrame],
  host: {
    '(document:keydown)': 'frame().handleKey($event)',
    '[attr.data-dialog]': 'id',
    '[attr.data-kind]': 'def?.kind',
    '[attr.data-modality]': 'modality()',
  },
  template: `
    <main class="pd-shell" [attr.aria-label]="title()">
      <pd-dialog-frame
        [dialogId]="id"
        [title]="title()"
        [kind]="def?.kind ?? 'form'"
        [modality]="modality()"
        [requiredChoice]="def?.requiredChoice === true"
        [state]="state()"
        [failure]="failure()"
        [content]="content()"
        [contentView]="contentView()"
        [declaredWidth]="def?.width ?? 0"
        (answered)="answer($event.value)"
        (dismissed)="dismiss()"
        (refit)="refit()"
      >
        @if (denied(); as d) {
          <p class="pd-denied" role="alert" data-testid="dialog-denied">{{ d.message }}</p>
        }
        <ng-container #host />
      </pd-dialog-frame>
    </main>
  `,
  styles: `
    :host { display: block; height: 100vh; background: var(--pd-surface-2); color: var(--pd-text); }
    .pd-shell { height: 100%; }
    .pd-denied { margin: 0; padding: 8px 10px; color: var(--pd-danger); border: 1px solid var(--pd-danger); border-radius: var(--pd-radius-control, 6px); overflow-wrap: anywhere; }
  `,
})
export class DialogShell implements AfterViewInit {
  private readonly route = inject(DIALOG_ROUTE);
  private readonly io = inject(DIALOG_IO);
  private readonly environment = inject(EnvironmentInjector);
  private readonly destroyRef = inject(DestroyRef);
  private readonly cdr = inject(ChangeDetectorRef);
  /** The start is a pending task: `whenStable` (tests, the boot harness) waits for it. */
  private readonly pending = inject(PendingTasks);
  private readonly app = inject(ApplicationRef);
  protected readonly frame = viewChild.required(DialogFrame);
  private readonly host = viewChild.required('host', { read: ViewContainerRef });
  protected readonly id = this.route?.id ?? '';
  protected readonly def: DialogDef | undefined = this.id ? inject(DIALOG_LOOKUP)(this.id) : undefined;
  private contentInjector_: Injector | null = null;
  private closing = false;
  private readied = false;
  /** This page load (`DialogStart.epoch`) and the loop that sizes the window (ADR 0043 §2). */
  private epoch = 1;
  private loop: SizeLoop | null = null;

  protected readonly title = signal('');
  protected readonly modality = signal<DialogModality>(this.def?.modality ?? 'modeless');
  protected readonly state = signal<FrameState>('loading');
  protected readonly failure = signal<string | null>(null);
  protected readonly content = signal<DialogContent | null>(null);
  protected readonly contentView = signal<{ detectChanges(): void } | null>(null);
  protected readonly denied = inject(DeniedCommands).last;

  /** What the content injects as `DIALOG_REF`: answering or dismissing closes this window. */
  readonly ref: DialogRef = {
    closed: new Promise<DialogOutcome<unknown>>(() => undefined),
    answer: (value) => void this.close(answered(value)),
    dismiss: () => this.frame().requestDismiss(),
    setTitle: (title) => this.setTitle(title),
    requestFit: () => this.frame().requestFit(),
  };

  constructor() {
    let off: (() => void) | null = null;
    let gone = false;
    this.io.listen(DIALOG_EVENTS.closeRequested, () => this.frame().requestDismiss()).then(
      (u) => (gone ? u() : (off = u)),
      () => undefined,
    );
    this.destroyRef.onDestroy(() => {
      gone = true;
      off?.();
    });
    // Rust's events (ADR 0043 §3, §4): the size mode changed (a user resize made it manual), and the scale changed so the text
    // may re-wrap: measure again.
    this.listenTo(DIALOG_EVENTS.sizeMode, (p) => {
      if (typeof document !== 'undefined' && isRecord(p) && typeof p['mode'] === 'string') document.documentElement.dataset['sizeMode'] = p['mode'];
    });
    this.listenTo(DIALOG_EVENTS.remeasure, () => this.refit());
  }

  private listenTo(event: string, handler: (payload: unknown) => void): void {
    let off: (() => void) | null = null;
    let gone = false;
    this.io.listen(event, handler).then(
      (u) => (gone ? u() : (off = u)),
      () => undefined,
    );
    this.destroyRef.onDestroy(() => {
      gone = true;
      off?.();
    });
  }

  /**
   * Starts as soon as the shell's own view exists, from the first change detection: never from `afterNextRender`, which a
   * hidden window may never reach (the 1.1-0 spike).
   */
  ngAfterViewInit(): void {
    const started = this.pending.add();
    this.destroyRef.onDestroy(started);
    void this.start()
      .finally(started)
      .then(() => this.settle());
  }

  private async start(): Promise<void> {
    const host = this.host();
    const frame = this.frame();
    try {
      const def = this.def;
      if (!def) throw new Error($localize`:@@pd.dialogs.unknown:This dialog is not known to this version of the app: ${this.id || '(none)'}:dialog:`);
      const [start, part] = await Promise.all([this.io.take(), loadPart(def, this.environment, `dialog ${def.id}`)]);
      if (part.injector !== this.environment) this.destroyRef.onDestroy(() => part.injector.destroy());
      this.epoch = start.epoch;
      this.title.set(start.title);
      if (typeof document !== 'undefined') document.title = start.title;
      const input = isRecord(start.input) ? start.input : {};
      const injector = contentInjector(part.injector, this.ref, input);
      const ref = mountContent(host, part.component, injector, part.injector, input);
      this.contentView.set(ref.changeDetectorRef);
      this.destroyRef.onDestroy(followDialogInputs(def.id, ref, this.io));
      this.content.set(ref.instance as DialogContent);
      this.contentInjector_ = injector;
      this.state.set('ready');
    } catch (e) {
      console.error(`[paddock-dialogs] dialog window ${this.id} failed to start`, e);
      this.failure.set(errorText(e));
      this.state.set('failed');
    }
    await this.ready(frame);
  }

  /**
   * The start (ADR 0043 §2): the content and the fonts are there; the frame is measured now, without waiting for a frame,
   * by the same loop that sizes the window afterwards. The window is shown at the measure the ready names.
   */
  private async ready(frame: DialogFrame): Promise<void> {
    const fonts = typeof document !== 'undefined' ? document.fonts?.ready : undefined;
    await (fonts ?? Promise.resolve()).catch(() => undefined);
    if (this.closing || this.readied) return;
    this.cdr.detectChanges();
    // Method d (the 1.1-0 spike): the frame is laid out at the contract's width and read synchronously, with no frame
    // wait; a font the content started to load meanwhile is waited for and the frame read once more.
    frame.pinWidth(this.def?.width ?? null);
    frame.render();
    frame.describe();
    frame.focusFirst();
    const loop = (this.loop = new SizeLoop(
      this.epoch,
      () => frame.measure(),
      (epoch, seq, measure) => this.io.measure(epoch, seq, measure),
      (message) => {
        console.warn(`[paddock-dialogs] dialog window ${this.id}: ${message}`);
        this.io.log?.('warn', `dialog window ${this.id}: ${message}`);
      },
      (seq) => {
        if (typeof document !== 'undefined') document.documentElement.dataset['fitSeq'] = String(seq);
        this.followUp();
      },
    ));
    loop.run('trigger');
    if (typeof document !== 'undefined' && document.fonts?.status === 'loading') {
      await document.fonts.ready.catch(() => undefined);
      loop.run('trigger');
    }
    this.readied = true;
    await this.io.ready(this.epoch, loop.seq).catch(() => undefined);
    // The content may have changed while the window system answered (a dialog that loads its data): the same loop reads it,
    // at the same pinned width, against the last measure it SENT; left to the watch (whose baseline would be read after
    // the pin is released) the window would keep its first, empty size.
    loop.run('trigger');
    // Shown at its size now: the window's own width rules again.
    frame.pinWidth(null);
    frame.watch();
  }

  /** The page's layout changed (content, settings, fonts, `requestFit`, a remeasure): the loop measures again. */
  protected refit(): void {
    if (this.readied && !this.closing) this.loop?.run('trigger');
  }

  /** After a send Rust resizes the window and the page re-lays out: one more look, a frame later (a visible window has frames). */
  private followUp(): void {
    if (typeof requestAnimationFrame !== 'function') return;
    // `data-fit-pending`: follow-ups scheduled and not yet run. A check waits for it to reach 0 (a condition), never for frames:
    // a window the OS does not paint (an occluded WebKit one) may not get a frame for seconds.
    const pending = (by: number) => {
      if (typeof document !== 'undefined') document.documentElement.dataset['fitPending'] = String(Math.max(0, Number(document.documentElement.dataset['fitPending'] ?? '0') + by));
    };
    pending(1);
    requestAnimationFrame(() => {
      pending(-1);
      if (!this.closing) this.loop?.run('follow-up');
    });
  }

  /**
   * The window is settled when the start is through and the content has nothing in flight (its stores and jobs hold a
   * pending task while they load): the last size the content reached is then sent at once, without a frame wait, and
   * `data-fit-settled` is set. A check that reads the window's size (the browser build's e2e) waits for it, so it never
   * measures a content that is still loading (File hash's result arriving after the first measure).
   */
  private async settle(): Promise<void> {
    if (!this.readied) return;
    await this.app.whenStable();
    if (this.closing) return;
    this.loop?.run('trigger');
    if (typeof document !== 'undefined') document.documentElement.dataset['fitSettled'] = 'true';
  }

  /** A title the content knows only now: the heading, the page title and (where the window system can) the OS title bar. */
  private setTitle(title: string): void {
    const text = title.trim();
    if (!text) return;
    this.title.set(text);
    if (typeof document !== 'undefined') document.title = text;
    this.io.setTitle?.(text).catch(() => undefined);
  }

  protected answer(value: unknown): void {
    void this.close(answered(value));
  }

  /** The frame's dismissal: the content may keep the dialog open (unsaved edits). */
  protected dismiss(): void {
    const content = this.content();
    if (this.contentInjector_ && content?.dialogCanClose?.(this.contentInjector_) === false) {
      this.frame().reopen();
      return;
    }
    void this.close(DISMISSED);
  }

  private async close(outcome: DialogOutcome<unknown>): Promise<void> {
    if (this.closing) return;
    const frame = this.frame();
    this.closing = true;
    try {
      await this.io.close(encodeOutcome(outcome));
    } catch (e) {
      this.closing = false;
      frame.reopen();
      console.error(`[paddock-dialogs] dialog window ${this.id} could not close: ${errorText(e)}`);
    }
  }
}
