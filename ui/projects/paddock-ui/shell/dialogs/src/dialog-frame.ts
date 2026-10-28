import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, ChangeDetectorRef, Component, computed, DestroyRef, ElementRef, inject, input, output, signal, viewChild } from '@angular/core';
import type { DialogKind, DialogModality } from '@decryp7/paddock-ui/shell';
import { DialogActionError, dialogKeyAction, dialogPlatform, layoutActions, type ActionLayout, type DialogAction, type DialogPlatform, type PlacedAction } from './actions';
import { DIALOG_MIN } from './kind';
import type { DialogMeasure } from './io';
import type { DialogContent } from './types';

/** How long a dialog waits for its ready handshake before it is shown anyway, Loading with Close (ADR 0026 §4.3). */
export const DIALOG_SHOW_DEADLINE_MS = 1000;

/** The state of a dialog's content. */
export type FrameState = 'loading' | 'ready' | 'failed';

/** An action that rejected: its user message, and the action Try again runs. */
interface Rejection {
  readonly message: string;
  readonly action: DialogAction;
}

/** The body's minimum when the content declares none: two lines. */
const MIN_BODY_LINES = 2;

const FIELD = 'input:not([type=hidden]):not([disabled]), select:not([disabled]), textarea:not([disabled]), [contenteditable=true]';

/**
 * Schedules `work` at most once per animation frame. A hidden window may never run `requestAnimationFrame` (the 1.1-0
 * spike: a sleeping Mac), so a hidden document takes the next task instead. Returns the asker and a cancel.
 */
export function oncePerFrame(work: () => void, doc: Pick<Document, 'visibilityState'> | undefined = globalThis.document): { readonly ask: () => void; readonly cancel: () => void } {
  let pending: { cancel: () => void } | null = null;
  const run = () => {
    pending = null;
    work();
  };
  return {
    ask: () => {
      if (pending) return;
      if (doc?.visibilityState === 'visible' && typeof requestAnimationFrame === 'function') {
        const id = requestAnimationFrame(run);
        pending = { cancel: () => cancelAnimationFrame(id) };
      } else {
        const id = setTimeout(run, 0);
        pending = { cancel: () => clearTimeout(id) };
      }
    },
    cancel: () => {
      pending?.cancel();
      pending = null;
    },
  };
}

/**
 * The frame of every dialog (ADR 0026 §2), in its own window (`DialogShell`) and in the page (`pd-dialog-host`): an
 * optional header, a body that alone scrolls (the content is projected into it) and a footer the frame draws from the
 * content's declared actions with `layoutActions`. It owns Enter, Escape and the first focus, the busy and rejected
 * states, and measures what the window needs, synchronously (`measure`), refitting at most once per frame.
 */
@Component({
  selector: 'pd-dialog-frame',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgTemplateOutlet],
  host: {
    class: 'pd-dialog-frame',
    '[attr.data-platform]': 'platform()',
    '[attr.data-kind]': 'kind()',
    '[attr.data-state]': 'state()',
    '[attr.aria-busy]': 'busy() !== null || state() === "loading"',
  },
  template: `
    @if (rejected(); as r) {
      <div class="pd-frame-alert" role="alert" data-testid="dialog-alert" #alert>
        <span class="pd-frame-alert-text" data-testid="dialog-alert-text">{{ r.message }}</span>
        <button #retryButton type="button" class="pd-frame-btn" data-testid="dialog-try-again" (click)="retry()">{{ t.tryAgain }}</button>
      </div>
    }
    @if (header(); as h) {
      <div class="pd-frame-head" data-testid="dialog-header" #head><ng-container [ngTemplateOutlet]="h" /></div>
    }
    <div
      class="pd-frame-body"
      #body
      data-testid="dialog-body"
      [attr.inert]="busy() !== null ? '' : null"
      [attr.role]="kind() === 'decision' ? 'alertdialog' : null"
      [attr.aria-label]="kind() === 'decision' ? title() : null"
      [attr.aria-describedby]="describedBy()"
    >
      <div class="pd-frame-content" #content>
        @if (state() === 'failed') {
          <div class="pd-frame-failed" role="alert" data-testid="dialog-error">
            <p class="pd-frame-failed-title">{{ t.openFailed }}</p>
            <p class="pd-frame-failed-reason" data-testid="dialog-error-reason">{{ failure() }}</p>
          </div>
        } @else if (state() === 'loading') {
          <p class="pd-frame-loading" data-testid="dialog-loading">{{ t.loading }}</p>
        }
        <ng-content />
      </div>
    </div>
    <div class="pd-frame-foot" #foot data-testid="dialog-footer">
      @if (statusLine(); as s) {
        <p class="pd-frame-status" role="status" data-testid="dialog-status" aria-live="polite">{{ s }}</p>
      }
      <div class="pd-frame-group" #lead>
        @for (p of layout().lead; track p.action.id) {
          <ng-container [ngTemplateOutlet]="button" [ngTemplateOutletContext]="{ $implicit: p }" />
        }
      </div>
      <span class="pd-frame-spacer"></span>
      <div class="pd-frame-group" #trail>
        @for (p of layout().trail; track p.action.id) {
          <ng-container [ngTemplateOutlet]="button" [ngTemplateOutletContext]="{ $implicit: p }" />
        }
      </div>
    </div>
    <ng-template #button let-p>
      <button
        type="button"
        class="pd-frame-btn"
        [class.pd-frame-btn--filled]="p.filled"
        [class.pd-frame-btn--danger]="p.danger"
        [attr.data-testid]="'dialog-action-' + p.action.id"
        [attr.data-role]="p.action.kind === 'dismiss' ? 'dismiss' : p.action.role"
        [disabled]="disabled(p)"
        [attr.aria-busy]="busy() === p.action.id"
        (click)="press(p.action)"
      >
        @if (busy() === p.action.id) {
          <span class="pd-frame-spinner" aria-hidden="true"></span>
        }
        {{ p.action.label }}
      </button>
    </ng-template>
  `,
  styles: `
    :host { box-sizing: border-box; display: flex; flex-direction: column; min-height: 0; height: 100%; color: var(--pd-text); }
    :host(.pd-frame--pinned) { flex: none; max-width: none; }
    .pd-frame-alert { flex: none; display: flex; align-items: center; gap: var(--pd-space-3, 12px); margin: var(--pd-space-3, 12px) var(--pd-space-4, 16px) 0; padding: var(--pd-space-2, 8px) var(--pd-space-3, 12px); border-radius: var(--pd-radius-control, 6px); border: 1px solid var(--pd-danger); background: var(--pd-danger-tint); }
    .pd-frame-alert-text { flex: 1 1 auto; min-width: 0; overflow-wrap: anywhere; }
    .pd-frame-head { flex: none; padding: var(--pd-space-3, 12px) var(--pd-space-4, 16px) 0; }
    .pd-frame-body { flex: 1 1 auto; min-height: 0; overflow: auto; padding: var(--pd-space-4, 16px); outline: none; }
    .pd-frame-content { display: flex; flex-direction: column; gap: var(--pd-space-3, 12px); min-width: 0; }
    :host([data-kind='tool']) .pd-frame-content { height: 100%; }
    :host([data-kind='tool']) .pd-frame-content > :not(.pd-frame-loading):not(.pd-frame-failed) { flex: 1 1 auto; min-height: 0; }
    .pd-frame-loading { margin: var(--pd-space-4, 16px) auto; color: var(--pd-text-muted); }
    .pd-frame-failed-title { margin: 0 0 var(--pd-space-2, 8px); font-size: var(--pd-text-subtitle, 15px); font-weight: var(--pd-weight-semibold, 600); }
    .pd-frame-failed-reason { margin: 0; color: var(--pd-danger); overflow-wrap: anywhere; }
    .pd-frame-foot { flex: none; display: flex; align-items: center; gap: var(--pd-space-2, 8px); padding: var(--pd-space-3, 12px) var(--pd-space-4, 16px); border-top: 1px solid var(--pd-hairline); background: var(--pd-surface-2); }
    .pd-frame-status { flex: 0 1 auto; min-width: 0; margin: 0; color: var(--pd-text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-frame-group { flex: none; display: flex; gap: var(--pd-space-2, 8px); }
    .pd-frame-spacer { flex: 1 1 auto; }
    .pd-frame-btn { display: inline-flex; align-items: center; gap: var(--pd-space-2, 8px); min-height: var(--pd-button-height, 28px); padding: 0 var(--pd-space-3, 12px); border-radius: var(--pd-radius-control, 6px); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium, 500); white-space: nowrap; cursor: pointer; }
    .pd-frame-btn--filled { background: var(--pd-accent); border-color: var(--pd-accent); color: var(--pd-on-accent); }
    .pd-frame-btn--danger { background: var(--pd-danger-fill); border-color: var(--pd-danger-fill); color: var(--pd-on-danger); }
    .pd-frame-btn:disabled { opacity: 0.55; cursor: default; }
    .pd-frame-btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-frame-spinner { inline-size: 12px; block-size: 12px; border-radius: 50%; border: 2px solid currentColor; border-inline-end-color: transparent; animation: pd-frame-spin 0.8s linear infinite; }
    @keyframes pd-frame-spin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { .pd-frame-spinner { animation: none; border-inline-end-color: currentColor; opacity: 0.6; } }
  `,
})
export class DialogFrame {
  /** The component's texts in the window's language (plan S15). */
  protected readonly t = {
    cancel: $localize`:@@pd.dialogs.cancel:Cancel`,
    close: $localize`:@@pd.dialogs.close:Close`,
    loading: $localize`:@@pd.dialogs.loading:Loading…`,
    openFailed: $localize`:@@pd.dialogs.open-failed:This window couldn't open`,
    tryAgain: $localize`:@@pd.dialogs.try-again:Try again`,
    finishing: $localize`:@@pd.dialogs.finishing:Finishing…`,
    failed: $localize`:@@pd.dialogs.action-failed:That didn't work. Try again, or close this window.`,
  };

  readonly dialogId = input('');
  readonly title = input('');
  readonly kind = input<DialogKind>('form');
  readonly modality = input<DialogModality>('modal');
  readonly requiredChoice = input(false);
  readonly state = input<FrameState>('loading');
  /** Why the content failed (state `failed`). */
  readonly failure = input<string | null>(null);
  /** The mounted content (its declared actions, status, header and minimum body height). */
  readonly content = input<DialogContent | null>(null);
  readonly platform = input<DialogPlatform>(dialogPlatform());
  /** The contract's width, logical px: what the frame is measured at. */
  readonly declaredWidth = input(0);

  /** The dialog answered. The host closes it with the value (no `dialogCanClose`: the content answered). */
  readonly answered = output<{ readonly value: unknown }>();
  /** The user dismissed it (Cancel/Close, Escape, Cmd/Ctrl+W, an OS close). The host asks `dialogCanClose` first. */
  readonly dismissed = output<void>();
  /** The content's natural size changed after the first measure (at most once per frame). */
  readonly refit = output<DialogMeasure>();

  private readonly cdr = inject(ChangeDetectorRef);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly destroyRef = inject(DestroyRef);
  private readonly bodyEl = viewChild.required<ElementRef<HTMLElement>>('body');
  private readonly contentEl = viewChild.required<ElementRef<HTMLElement>>('content');
  private readonly footEl = viewChild.required<ElementRef<HTMLElement>>('foot');
  private readonly leadEl = viewChild.required<ElementRef<HTMLElement>>('lead');
  private readonly trailEl = viewChild.required<ElementRef<HTMLElement>>('trail');

  /** The action whose work is running. */
  protected readonly busy = signal<string | null>(null);
  protected readonly rejected = signal<Rejection | null>(null);
  /** A dismissal waits for a non-abortable action to settle (the signal draws "Finishing…"; the field is read after awaits). */
  private readonly finishing = signal(false);
  private finishingNow = false;
  protected readonly describedBy = signal<string | null>(null);
  private controller: AbortController | null = null;
  private done = false;
  private watching = false;
  private destroyed = false;

  /** The footer as laid out now. While loading or after a failure: one Cancel/Close, so there is always a way out. */
  readonly layout = computed<ActionLayout>(() => {
    const state = this.state();
    const labels = { cancel: this.t.cancel, close: this.t.close };
    if (state !== 'ready') return layoutActions(this.kind(), state === 'failed' ? 'modeless' : this.modality(), false, [], this.platform(), labels);
    const actions = this.content()?.dialogActions?.() ?? [];
    const layout = layoutActions(this.kind(), this.modality(), this.requiredChoice(), actions, this.platform(), labels);
    if (layout.problems.length) console.error(`[paddock-dialogs] dialog ${this.dialogId()} declares invalid actions: ${layout.problems.join('; ')}`);
    return layout;
  });
  protected readonly header = computed(() => (this.state() === 'ready' ? (this.content()?.dialogHeader?.() ?? null) : null));
  protected readonly statusLine = computed(() => (this.finishing() ? this.t.finishing : this.state() === 'ready' ? (this.content()?.dialogStatus?.() ?? null) : null));

  constructor() {
    this.destroyRef.onDestroy(() => {
      this.destroyed = true;
      this.controller?.abort();
    });
  }

  protected disabled(p: PlacedAction): boolean {
    const busy = this.busy();
    if (busy === null) return p.action.enabled === false;
    return !(p.action.kind === 'dismiss' && this.abortable(busy));
  }

  private abortable(id: string): boolean {
    const a = this.find(id);
    return a?.kind === 'answer' && a.abortable === true;
  }

  private find(id: string | null): DialogAction | undefined {
    if (id === null) return undefined;
    const l = this.layout();
    return [...l.lead, ...l.trail].find((p) => p.action.id === id)?.action;
  }

  /** Runs an action: an answer or a run with the busy and rejected states, a dismiss as a dismissal. */
  press(action: DialogAction): void {
    if (this.done) return;
    if (action.kind === 'dismiss') return this.requestDismiss(true);
    if (this.busy() !== null || action.enabled === false) return;
    void this.work(action);
  }

  private async work(action: Exclude<DialogAction, { kind: 'dismiss' }>): Promise<void> {
    const dialogId = this.dialogId();
    const controller = new AbortController();
    this.controller = controller;
    this.rejected.set(null);
    this.busy.set(action.id);
    this.render();
    try {
      const value = action.kind === 'answer' ? await action.answer(controller.signal) : await action.run(controller.signal);
      if (controller.signal.aborted || this.done) return;
      if (action.kind === 'answer') return this.finish({ value });
      this.busy.set(null);
      if (this.finishingNow) return this.finish(null);
    } catch (e) {
      if (controller.signal.aborted || this.done) return;
      this.busy.set(null);
      if (this.finishingNow) return this.finish(null);
      console.error(`[paddock-dialogs] action ${action.id} of dialog ${dialogId} failed`, e instanceof DialogActionError ? 'DialogActionError' : (e as Error | null)?.name);
      this.rejected.set({ message: e instanceof DialogActionError && e.message.trim() ? e.message : this.t.failed, action });
      this.render();
      this.host.nativeElement.querySelector<HTMLElement>('[data-testid="dialog-try-again"]')?.focus();
    } finally {
      if (this.controller === controller) this.controller = null;
      this.render();
    }
  }

  /** Try again: the rejected action, once more. */
  protected retry(): void {
    const r = this.rejected();
    if (r) this.press(r.action);
  }

  private finish(answer: { readonly value: unknown } | null): void {
    this.done = true;
    this.busy.set(null);
    this.finishing.set(false);
    this.finishingNow = false;
    if (answer) this.answered.emit(answer);
    else this.dismissed.emit();
  }

  /**
   * Cancel/Close, Escape, Cmd/Ctrl+W or an OS close request. A required choice ignores it, unless its content failed, or
   * the user pressed the Close shown while it loads (`fromButton`: a stray Escape during loading is still ignored). While
   * work runs: an abortable action is aborted and the dialog dismissed at once; otherwise the
   * dismissal waits until the work settles ("Finishing…"), and an answer that arrives meanwhile still closes as answered.
   */
  requestDismiss(fromButton = false): void {
    if (this.done) return;
    if (this.requiredChoice() && this.state() !== 'failed' && !(fromButton && this.layout().dismiss !== null)) return;
    const busy = this.busy();
    if (busy !== null) {
      if (this.abortable(busy)) {
        this.controller?.abort();
        this.finish(null);
      } else {
        this.finishing.set(true);
        this.finishingNow = true;
        this.render();
      }
      return;
    }
    this.dismissed.emit();
  }

  /** The host's dismissal was refused (`dialogCanClose`): the frame stays usable. */
  reopen(): void {
    this.done = false;
  }

  /** A key press in the dialog: Escape and Cmd/Ctrl+W dismiss, Enter runs the Enter target (never a destructive action). */
  handleKey(e: KeyboardEvent): void {
    const k = dialogKeyAction(e);
    if (k === 'dismiss') {
      e.preventDefault();
      this.requestDismiss();
    } else if (k === 'enter') {
      const target = this.find(this.layout().enterTarget);
      if (!target || target.enabled === false || this.busy() !== null) return;
      e.preventDefault();
      this.press(target);
    }
  }

  /** Renders the frame now: a hidden window may never run a frame, so nothing here waits for one (the 1.1-0 spike). */
  render(): void {
    if (!this.destroyed) this.cdr.detectChanges();
  }

  /** Gives the first focus: a form's first field (one the content marks `data-autofocus` first), else the action the layout names. */
  focusFirst(): void {
    this.render();
    const focus = this.layout().focus;
    const body = this.bodyEl().nativeElement;
    if (focus === 'field') {
      const field = body.querySelector<HTMLElement>('[data-autofocus]') ?? body.querySelector<HTMLElement>(FIELD);
      if (field) return field.focus();
      return this.focusAction(this.layout().enterTarget ?? this.layout().dismiss?.id ?? null);
    }
    this.focusAction(focus);
  }

  private focusAction(id: string | null): void {
    if (id === null) return;
    this.host.nativeElement.querySelector<HTMLElement>(`[data-testid="dialog-action-${id}"]`)?.focus();
  }

  /** Names the decision's first paragraph as its description (ADR 0026 §2, accessibility). */
  describe(): void {
    if (this.kind() !== 'decision') return;
    const p = this.contentEl().nativeElement.querySelector<HTMLElement>('p');
    if (!p) return;
    p.id ||= `pd-dialog-desc-${this.dialogId().replace(/[^a-z0-9-]/g, '-') || 'x'}`;
    this.describedBy.set(p.id);
    this.render();
  }

  /**
   * What the dialog needs, measured now and synchronously (method d of the 1.1-0 spike): the header, the body at its
   * natural height and the footer at the frame's width; the minimum is the header, the footer and the body's declared
   * minimum, at the width of the footer's buttons on one line.
   */
  measure(): DialogMeasure {
    this.render();
    const height = (el: Element | null | undefined) => (el ? el.getBoundingClientRect().height : 0);
    const host = this.host.nativeElement;
    const body = this.bodyEl().nativeElement;
    const style = getComputedStyle(body);
    const padY = (parseFloat(style.paddingTop) || 0) + (parseFloat(style.paddingBottom) || 0);
    const line = parseFloat(style.lineHeight) || 20;
    const chrome = height(host.querySelector('.pd-frame-alert')) + height(host.querySelector('.pd-frame-head')) + height(this.footEl().nativeElement) + padY;
    const content = Math.ceil(chrome + height(this.contentEl().nativeElement));
    const minBody = this.content()?.dialogMinBodyHeight ?? line * MIN_BODY_LINES;
    const foot = getComputedStyle(this.footEl().nativeElement);
    const footPad = (parseFloat(foot.paddingLeft) || 0) + (parseFloat(foot.paddingRight) || 0);
    const gap = parseFloat(foot.columnGap) || 0;
    const buttons = this.leadEl().nativeElement.getBoundingClientRect().width + this.trailEl().nativeElement.getBoundingClientRect().width + gap * 2 + footPad;
    // The width is the contract's (pinned while measuring), never what a hidden or not yet sized window happens to be.
    const width = this.pinnedWidth ?? this.declaredWidth();
    return {
      content: { width: Math.ceil(width), height: content },
      minContent: { width: Math.ceil(Math.max(buttons, DIALOG_MIN.width)), height: Math.ceil(Math.min(content, chrome + minBody)) },
      viewportHeight: typeof window === 'undefined' ? undefined : window.innerHeight || undefined,
    };
  }

  /**
   * Lays the frame out at `width` logical px (the contract's) for measuring, whatever the window's viewport is now
   * (method d of the 1.1-0 spike: a hidden window may report any width, even 0). CSS zoom on the document is undone.
   * `null` releases it once the window is shown at its size.
   */
  pinWidth(width: number | null): void {
    const host = this.host.nativeElement;
    const zoom = typeof document === 'undefined' ? 1 : parseFloat(getComputedStyle(document.documentElement).zoom) || 1;
    this.pinnedWidth = width;
    if (width === null) host.style.removeProperty('width');
    else host.style.setProperty('width', `${width / zoom}px`);
    host.classList.toggle('pd-frame--pinned', width !== null);
  }

  private pinnedWidth: number | null = null;

  /**
   * Watches what changes the size after the first measure (ADR 0026 §2): the content's natural size, the footer and
   * alert, the fonts, the theme, density and font settings on `<html>`, and `requestFit()`; at most one refit per frame.
   */
  watch(): void {
    if (this.watching) return;
    this.watching = true;
    let last = '';
    const fit = oncePerFrame(() => {
      const m = this.measure();
      const key = `${m.content.width}x${m.content.height}/${m.minContent.width}x${m.minContent.height}`;
      if (key === last) return;
      last = key;
      this.refit.emit(m);
    });
    last = ((m) => `${m.content.width}x${m.content.height}/${m.minContent.width}x${m.minContent.height}`)(this.measure());
    this.requestFit = fit.ask;
    const observers: { disconnect(): void }[] = [];
    if (typeof ResizeObserver !== 'undefined') {
      const resize = new ResizeObserver(fit.ask);
      resize.observe(this.contentEl().nativeElement);
      resize.observe(this.footEl().nativeElement);
      observers.push(resize);
    }
    if (typeof MutationObserver !== 'undefined' && typeof document !== 'undefined') {
      const settings = new MutationObserver(fit.ask);
      settings.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-density', 'data-font-size', 'style', 'class'] });
      const own = new MutationObserver(fit.ask);
      own.observe(this.host.nativeElement, { childList: true, subtree: false });
      observers.push(settings, own);
    }
    const fonts = typeof document !== 'undefined' ? document.fonts : undefined;
    fonts?.addEventListener?.('loadingdone', fit.ask);
    this.destroyRef.onDestroy(() => {
      fit.cancel();
      for (const o of observers) o.disconnect();
      fonts?.removeEventListener?.('loadingdone', fit.ask);
    });
  }

  /** Asks for a refit (the content changed what it shows without a resize the frame sees). A no-op before `watch`. */
  requestFit: () => void = () => undefined;
}
