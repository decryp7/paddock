import { CdkTrapFocus } from '@angular/cdk/a11y';
import {
  type AfterViewInit,
  ChangeDetectionStrategy,
  ChangeDetectorRef,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  EnvironmentInjector,
  inject,
  input,
  PendingTasks,
  signal,
  viewChild,
  ViewContainerRef,
} from '@angular/core';
import { loadPart } from '@decryp7/paddock-ui/shell';
import { DIALOG_SHOW_DEADLINE_MS, DialogFrame, type FrameState } from './dialog-frame';
import { InPageDialogBackend, type InPageDialog } from './in-page';
import { resolveSize } from './kind';
import { ModalGate } from './modal-gate';
import { contentInjector, errorText, mountContent } from './mount';
import { followDialogInputs } from './retarget';
import { DialogPresenter, type DialogContent } from './types';

/**
 * One dialog in the page (ADR 0026 §4.8): a title bar, then the same `DialogFrame` as a dialog window, so the footer,
 * keys, busy and rejected states are identical. It stays hidden until the frame is ready (content and fonts) or the show
 * deadline passed (then Loading with its Close), and a modal one traps the focus (CDK `cdkTrapFocus`).
 */
@Component({
  selector: 'pd-in-page-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DialogFrame, CdkTrapFocus],
  host: {
    '[attr.role]': "entry().def.kind === 'decision' ? 'alertdialog' : 'dialog'",
    class: 'pd-in-page-dialog',
    '[class.pd-modeless]': "entry().def.modality === 'modeless'",
    '[class.pd-pending]': '!shown()',
    '[attr.aria-modal]': "entry().def.modality === 'modal'",
    '[attr.aria-labelledby]': 'labelledBy()',
    '[attr.aria-describedby]': "entry().def.kind === 'decision' ? frame().describedBy() : null",
    '[attr.data-dialog]': 'entry().def.id',
    '[attr.data-kind]': 'entry().def.kind',
    '[attr.data-modality]': 'entry().def.modality',
    '[style.width.px]': 'width()',
    '[style.height.px]': 'height()',
    '(keydown)': 'onKey($event)',
  },
  template: `
    <div class="pd-trap" [cdkTrapFocus]="entry().def.modality === 'modal'">
      <header class="pd-head">
        <h2 class="pd-title" [id]="titleId()">{{ entry().title() }}</h2>
      </header>
      <pd-dialog-frame
        [dialogId]="entry().def.id"
        [title]="entry().title()"
        [kind]="entry().def.kind"
        [modality]="entry().def.modality"
        [requiredChoice]="entry().def.requiredChoice"
        [state]="state()"
        [failure]="failure()"
        [content]="content()"
        [declaredWidth]="width()"
        [ownsRole]="false"
        [headingId]="headingId()"
        (answered)="entry().ref.answer($event.value)"
        (dismissed)="dismissed()"
      >
        <ng-container #contentHost />
      </pd-dialog-frame>
    </div>
  `,
  styles: `
    :host { position: fixed; top: 56px; inset-inline-start: 50%; transform: translateX(-50%); max-width: calc(100vw - 32px); max-height: calc(100vh - 96px); box-sizing: border-box; display: flex; flex-direction: column; border-radius: var(--pd-radius-lg, 14px); background: var(--pd-surface-2); color: var(--pd-text); border: 1px solid var(--pd-hairline); box-shadow: var(--pd-shadow-3); overflow: hidden; }
    :host(.pd-modeless) { top: 72px; inset-inline-start: auto; inset-inline-end: 24px; transform: none; max-height: calc(100vh - 112px); }
    :host(.pd-pending) { visibility: hidden; }
    .pd-trap { display: flex; flex-direction: column; min-height: 0; flex: 1 1 auto; }
    .pd-head { flex: none; display: flex; align-items: center; padding: var(--pd-space-2, 8px) var(--pd-space-4, 16px); border-bottom: 1px solid var(--pd-hairline); }
    .pd-title { margin: 0; font-size: var(--pd-text-subtitle, 15px); font-weight: var(--pd-weight-semibold, 600); overflow-wrap: anywhere; }
    pd-dialog-frame { flex: 1 1 auto; min-height: 0; }
  `,
})
export class InPageDialogView implements AfterViewInit {
  readonly entry = input.required<InPageDialog>();
  private readonly environment = inject(EnvironmentInjector);
  private readonly destroyRef = inject(DestroyRef);
  private readonly cdr = inject(ChangeDetectorRef);
  private readonly pendingTasks = inject(PendingTasks);
  private readonly element = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly host = viewChild.required('contentHost', { read: ViewContainerRef });
  protected readonly frame = viewChild.required(DialogFrame);
  protected readonly state = signal<FrameState>('loading');
  protected readonly failure = signal<string | null>(null);
  protected readonly content = signal<DialogContent | null>(null);
  /** Shown once the frame is ready or the show deadline passed (ADR 0026 §4.3). */
  protected readonly shown = signal(false);
  protected readonly titleId = computed(() => `pd-dialog-title-${this.entry().n}`);
  protected readonly headingId = computed(() => `pd-dialog-heading-${this.entry().n}`);
  /** A decision is named by its question once it has one, else by the title bar. */
  protected readonly labelledBy = computed(() => (this.entry().def.kind === 'decision' && this.content()?.dialogHeading?.() ? this.headingId() : this.titleId()));
  private readonly size = computed(() => resolveSize(this.entry().def));
  protected readonly width = computed(() => this.size().width);
  /** A tool keeps its height; the other kinds take what their content needs (bounded by the page). */
  protected readonly height = computed(() => (this.entry().def.kind === 'tool' ? this.size().height : undefined));

  ngAfterViewInit(): void {
    const entry = this.entry();
    entry.requestDismiss = () => this.frame().requestDismiss();
    entry.requestFit = () => this.frame().requestFit();
    const deadline = setTimeout(() => this.show(), DIALOG_SHOW_DEADLINE_MS);
    this.destroyRef.onDestroy(() => clearTimeout(deadline));
    // The start is a pending task: `whenStable` (specs) waits until the content is shown.
    const started = this.pendingTasks.add();
    void this.load().then(() => {
      clearTimeout(deadline);
      started();
    });
  }

  private async load(): Promise<void> {
    const entry = this.entry();
    const host = this.host();
    const frame = this.frame();
    try {
      const part = await loadPart(entry.def, this.environment, `dialog ${entry.def.id}`);
      if (part.injector !== this.environment) this.destroyRef.onDestroy(() => part.injector.destroy());
      const injector = contentInjector(part.injector, entry.ref, entry.inputs);
      const ref = mountContent(host, part.component, injector, part.injector, entry.inputs);
      this.destroyRef.onDestroy(followDialogInputs(entry.def.id, ref));
      entry.content = ref.instance as DialogContent;
      entry.injector = injector;
      this.content.set(entry.content);
      this.state.set('ready');
    } catch (e) {
      console.error(`[paddock-dialogs] dialog ${entry.def.id} failed to start`, e);
      this.failure.set(errorText(e));
      this.state.set('failed');
    }
    const fonts = typeof document !== 'undefined' ? document.fonts?.ready : undefined;
    await (fonts ?? Promise.resolve()).catch(() => undefined);
    this.show();
    frame.describe();
    frame.focusFirst();
  }

  private show(): void {
    this.shown.set(true);
    // Now, not at the parent's next check: the first focus follows at once and a hidden element cannot take it.
    this.element.nativeElement.classList.remove('pd-pending');
    this.cdr.detectChanges();
  }

  /** The frame's dismissal; the content may keep the dialog open. */
  protected dismissed(): void {
    if (!this.entry().dismissNow()) this.frame().reopen();
  }

  protected onKey(e: KeyboardEvent): void {
    this.frame().handleKey(e);
    if (e.defaultPrevented) e.stopPropagation();
  }
}

/**
 * Renders the dialogs this window shows in the page, and the scrim over the window while one of its modal dialog
 * windows is open (on macOS the OS does not disable the parent, ADR 0009). Put one in the window's root template.
 */
@Component({
  selector: 'pd-dialog-host',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [InPageDialogView],
  host: { '(document:keydown.escape)': 'onEscape($event)' },
  template: `
    @if (scrim()) {
      <div class="pd-scrim" data-testid="dialog-scrim" aria-hidden="true"></div>
    }
    @for (d of stack(); track d.n; let i = $index) {
      @if (d.def.modality === 'modal') {
        <div class="pd-backdrop" data-testid="dialog-backdrop" [style.z-index]="210 + 2 * i" (mousedown)="dismiss(d)"></div>
      }
      <pd-in-page-dialog [entry]="d" [style.z-index]="211 + 2 * i" />
    }
  `,
  styles: `
    .pd-scrim, .pd-backdrop { position: fixed; inset: 0; background: var(--pd-scrim); }
    .pd-scrim { z-index: 200; }
  `,
})
export class DialogHost {
  private readonly inPage = inject(InPageDialogBackend);
  private readonly presenter = inject(DialogPresenter, { optional: true });
  private readonly gate = inject(ModalGate);
  protected readonly stack = this.inPage.stack;
  /** A modal dialog window (not one in the page, which has its own backdrop) is open over this window. */
  protected readonly scrim = computed(() => this.presenter?.windowed === true && this.gate.blocked() && !this.stack().some((d) => d.def.modality === 'modal'));

  /** Escape from outside any dialog answers the topmost modal (a modeless one answers only from inside it). */
  protected onEscape(e: Event): void {
    if (e.defaultPrevented) return;
    const top = this.stack().filter((d) => d.def.modality === 'modal').at(-1);
    if (!top) return;
    e.preventDefault();
    this.dismiss(top);
  }

  /** Escape and a click on the backdrop mean Cancel, with the frame's rules (a required choice, busy work). */
  protected dismiss(d: InPageDialog): void {
    d.ref.dismiss();
  }
}
