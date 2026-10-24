import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  EnvironmentInjector,
  inject,
  Injector,
  signal,
  viewChild,
  ViewContainerRef,
} from '@angular/core';
import { DIALOG_EVENTS, loadPart, type DialogDef, type DialogModality } from '@decryp7/paddock-ui/shell';
import { DeniedCommands } from './denied';
import { DIALOG_IO } from './io';
import { dialogKeyAction, findDefaultAction, hasCloseAction } from './keys';
import { fitsContent } from './kind';
import { contentInjector, errorText, isRecord, mountContent } from './mount';
import { DIALOG_ROUTE } from './route';
import { DIALOG_LOOKUP, type DialogContent, type DialogRef } from './types';

/**
 * Runs `work` once on the next frame however often it is asked for until then (a burst of DOM changes or resizes is
 * one check), with no delay of its own: the UI holds no timers on work (rule 12). Returns the asker and a cancel.
 */
export function coalesced(work: () => void): { readonly ask: () => void; readonly cancel: () => void } {
  let pending: number | ReturnType<typeof setTimeout> | null = null;
  const raf = typeof requestAnimationFrame === 'function';
  const run = () => {
    pending = null;
    work();
  };
  return {
    ask: () => {
      if (pending === null) pending = raf ? requestAnimationFrame(run) : setTimeout(run, 0);
    },
    cancel: () => {
      if (pending === null) return;
      if (raf) cancelAnimationFrame(pending as number);
      else clearTimeout(pending);
      pending = null;
    },
  };
}
/** The body's padding when the computed style cannot be read. */
const BODY_PADDING_PX = 16;

/**
 * The small app a dialog window boots into (ADR 0009, rule 34): the dialog's content under the OS title bar, which has
 * no buttons. It takes its input from Rust, loads the dialog's component with only its declared providers, answers
 * through `DIALOG_REF.close`, closes on Escape / Cmd+W and a close request (asking the content first; a required choice
 * ignores them), runs the default action on Enter, fits the window to its content by kind, and guarantees a visible
 * Close/Cancel in every state (loading, failed, content without one).
 */
@Component({
  selector: 'pd-dialog-shell',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '(document:keydown)': 'onKey($event)',
    '[attr.data-dialog]': 'id',
    '[attr.data-kind]': 'def?.kind',
    '[attr.data-modality]': 'modality()',
  },
  template: `
    <main #body class="body" tabindex="-1" [attr.aria-label]="title()" [attr.aria-busy]="loading()">
      @if (error(); as e) {
        <div class="failed" role="alert" data-testid="dialog-error">
          <h1 class="failed-title">{{ t.openFailed }}</h1>
          <p class="failed-reason" data-testid="dialog-error-reason">{{ e }}</p>
        </div>
      } @else if (loading()) {
        <p class="loading" data-testid="dialog-loading">{{ t.loading }}</p>
      }
      @if (denied(); as d) {
        <p class="denied" role="alert" data-testid="dialog-denied">{{ d.message }}</p>
      }
      <ng-container #host />
      @if (shellClose()) {
        <div class="foot" data-testid="dialog-shell-foot">
          <button type="button" class="pd-dialog-button" (click)="dismiss()" data-testid="dialog-shell-close" data-dialog-close>
            {{ modality() === 'modal' && !error() ? t.cancel : t.close }}
          </button>
        </div>
      }
    </main>
  `,
  styles: `
    :host { display: block; height: 100vh; background: var(--pd-surface-2); color: var(--pd-text); }
    .body { box-sizing: border-box; height: 100%; min-height: 0; overflow: auto; padding: 16px; display: flex; flex-direction: column; gap: 12px; outline: none; }
    .body > :not(.loading):not(.failed):not(.denied):not(.foot) { flex: 1 1 auto; min-height: 0; }
    .loading { margin: auto; color: var(--pd-text-muted); }
    .failed { margin: auto 0; }
    .failed-title { margin: 0 0 8px; font-size: var(--pd-text-subtitle, 15px); font-weight: var(--pd-weight-semibold, 600); }
    .failed-reason, .denied { margin: 0; color: var(--pd-danger); overflow-wrap: anywhere; }
    .denied { padding: 8px 10px; border: 1px solid var(--pd-danger); border-radius: var(--pd-radius-control, 6px); }
    .foot { display: flex; justify-content: flex-end; gap: 8px; }
    .pd-dialog-button { min-height: var(--pd-button-height, 28px); padding: 0 var(--pd-space-3, 12px); border-radius: var(--pd-radius-control, 6px); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium, 500); }
  `,
})
export class DialogShell {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    openFailed: $localize`:@@pd.dialogs.open-failed:This window couldn't open`,
    loading: $localize`:@@pd.dialogs.loading:Loading…`,
    cancel: $localize`:@@pd.dialogs.cancel:Cancel`,
    close: $localize`:@@pd.dialogs.close:Close`,
  };
  private readonly route = inject(DIALOG_ROUTE);
  private readonly io = inject(DIALOG_IO);
  private readonly environment = inject(EnvironmentInjector);
  private readonly injector = inject(Injector);
  private readonly destroyRef = inject(DestroyRef);
  private readonly body = viewChild.required<ElementRef<HTMLElement>>('body');
  private readonly host = viewChild.required('host', { read: ViewContainerRef });
  protected readonly id = this.route?.id ?? '';
  protected readonly def: DialogDef | undefined = this.id ? inject(DIALOG_LOOKUP)(this.id) : undefined;
  private content: DialogContent | null = null;
  private contentInjector_: Injector | null = null;
  private closing = false;
  /** Fits the window again after a short delay; set once the window is being fitted. */
  private refit: (() => void) | null = null;

  protected readonly title = signal('');
  protected readonly modality = signal<DialogModality>(this.def?.modality ?? 'modeless');
  protected readonly loading = signal(true);
  protected readonly error = signal<string | null>(null);
  /** Every state of a window without title-bar buttons needs a visible way out: the shell adds one when the content has none. */
  protected readonly shellClose = signal(false);
  protected readonly denied = inject(DeniedCommands).last;

  /** What the content injects as `DIALOG_REF`: closing answers the opener and ends this window. */
  readonly ref: DialogRef = { closed: new Promise(() => undefined), close: (result) => void this.close(result) };

  constructor() {
    let off: (() => void) | null = null;
    let gone = false;
    this.io.listen(DIALOG_EVENTS.closeRequested, () => void this.dismiss()).then(
      (u) => (gone ? u() : (off = u)),
      () => undefined,
    );
    this.destroyRef.onDestroy(() => {
      gone = true;
      off?.();
    });
    // The content is mounted once the shell's own view exists (its host container is a view query).
    afterNextRender(() => {
      this.body().nativeElement.focus();
      this.watchForCloseAction();
      void this.start();
    });
  }

  private async start(): Promise<void> {
    try {
      const def = this.def;
      if (!def) throw new Error($localize`:@@pd.dialogs.unknown:This dialog is not known to this version of the app: ${this.id || '(none)'}:dialog:`);
      const [start, part] = await Promise.all([this.io.take(), loadPart(def, this.environment, `dialog ${def.id}`)]);
      if (part.injector !== this.environment) this.destroyRef.onDestroy(() => part.injector.destroy());
      this.title.set(start.title);
      if (typeof document !== 'undefined') document.title = start.title;
      const input = isRecord(start.input) ? start.input : {};
      const injector = contentInjector(part.injector, this.ref, input);
      const ref = mountContent(this.host(), part.component, injector, part.injector, input);
      this.content = ref.instance as DialogContent;
      this.contentInjector_ = injector;
      this.loading.set(false);
      this.check();
      if (fitsContent(def)) this.fitToContent();
    } catch (e) {
      this.fail(e);
    }
  }

  private fail(e: unknown): void {
    console.error(`[paddock-dialogs] dialog window ${this.id} failed to start`, e);
    this.loading.set(false);
    this.error.set(errorText(e));
    this.check();
  }

  /** Adds or removes the shell's Close: needed when the content (or the error state) has no Close/Cancel of its own. */
  private check(): void {
    const body = this.body().nativeElement;
    const required = this.def?.requiredChoice === true && !this.error();
    const has = hasCloseAction(body, body.querySelector('[data-testid=dialog-shell-foot]'));
    const next = !required && !has;
    if (next === this.shellClose()) return;
    this.shellClose.set(next);
    // The footer changes how tall the window must be: fit again once it is drawn.
    this.refit?.();
  }

  private watchForCloseAction(): void {
    this.check();
    if (typeof MutationObserver === 'undefined') return;
    const recheck = coalesced(() => this.check());
    const observer = new MutationObserver(recheck.ask);
    observer.observe(this.body().nativeElement, { childList: true, subtree: true, characterData: true });
    this.destroyRef.onDestroy(() => {
      observer.disconnect();
      recheck.cancel();
    });
  }

  /**
   * Fits the window to what its content needs: once after the first layout and the fonts (fitting before them and again
   * after made the window jump), and again when the content grows (once per frame). Only grows: a window never shrinks under a
   * reader. Rust bounds the height by the dialog's floor and its monitor.
   */
  private fitToContent(): void {
    let last = 0;
    const fit = () => {
      const needed = this.neededHeight();
      if (needed <= 0 || needed <= last) return;
      last = needed;
      void this.io.fitHeight(needed).catch(() => undefined);
    };
    const refit = coalesced(fit);
    const schedule = refit.ask;
    this.refit = schedule;
    afterNextRender(
      () => {
        this.neededHeight();
        const fonts = typeof document !== 'undefined' ? document.fonts?.ready : undefined;
        void (fonts ?? Promise.resolve()).then(fit, fit);
        const content = this.body().nativeElement.firstElementChild;
        if (content && typeof ResizeObserver !== 'undefined') {
          const observer = new ResizeObserver(schedule);
          observer.observe(content);
          this.destroyRef.onDestroy(() => observer.disconnect());
        }
        this.destroyRef.onDestroy(refit.cancel);
      },
      { injector: this.injector },
    );
  }

  /** The window height the content needs: the last child's natural bottom edge plus the body's bottom padding, rounded up. */
  private neededHeight(): number {
    const body = this.body().nativeElement;
    const content = body.firstElementChild as HTMLElement | null;
    if (!content) return 0;
    // Measured at the natural height: the content normally stretches to fill the window.
    const flex = content.style.flex;
    content.style.flex = 'none';
    const last = (body.lastElementChild as HTMLElement | null) ?? content;
    const bottom = last.getBoundingClientRect().bottom - body.getBoundingClientRect().top + body.scrollTop;
    content.style.flex = flex;
    const padding = parseFloat(getComputedStyle(body).paddingBottom) || BODY_PADDING_PX;
    return Math.ceil(bottom + padding);
  }

  /** Closes this dialog with `result` (none: dismissed), unless the content refuses (unsaved edits). */
  protected async close(result?: unknown): Promise<void> {
    if (this.closing) return;
    if (this.contentInjector_ && this.content?.dialogCanClose?.(this.contentInjector_) === false) return;
    this.closing = true;
    try {
      await this.io.close(result);
    } catch (e) {
      this.closing = false;
      console.error(`[paddock-dialogs] dialog window ${this.id} could not close: ${errorText(e)}`);
    }
  }

  /** Escape, Cmd/Ctrl+W and a close request: the dialog's Cancel/Close. A required choice has none, unless it failed to open. */
  protected dismiss(): Promise<void> {
    return this.def?.requiredChoice && !this.error() ? Promise.resolve() : this.close();
  }

  protected onKey(e: KeyboardEvent): void {
    const action = dialogKeyAction(e);
    if (action === 'dismiss') {
      e.preventDefault();
      void this.dismiss();
    } else if (action === 'default') {
      const button = findDefaultAction(this.body().nativeElement);
      if (button) {
        e.preventDefault();
        button.click();
      }
    }
  }
}
