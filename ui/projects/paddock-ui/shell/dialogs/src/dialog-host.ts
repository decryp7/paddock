import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  EnvironmentInjector,
  inject,
  input,
  signal,
  untracked,
  viewChild,
  ViewContainerRef,
} from '@angular/core';
import { loadPart } from '@decryp7/paddock-ui/shell';
import { InPageDialogBackend, type InPageDialog } from './in-page';
import { dialogKeyAction, findDefaultAction } from './keys';
import { resolveSize } from './kind';
import { ModalGate } from './modal-gate';
import { contentInjector, errorText, mountContent } from './mount';
import { DialogPresenter, type DialogContent } from './types';

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** One dialog in the page: its title bar with Close, its content (loaded with its providers), Escape, Enter and focus. */
@Component({
  selector: 'pd-in-page-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    role: 'dialog',
    class: 'pd-in-page-dialog',
    '[class.modeless]': "entry().def.modality === 'modeless'",
    '[attr.aria-modal]': "entry().def.modality === 'modal'",
    '[attr.aria-labelledby]': 'titleId()',
    '[attr.data-dialog]': 'entry().def.id',
    '[attr.data-kind]': 'entry().def.kind',
    '[attr.data-modality]': 'entry().def.modality',
    '[style.width.px]': 'width()',
    '[style.height.px]': 'height()',
    '(keydown)': 'onKey($event)',
  },
  template: `
    <header class="head">
      <h2 class="title" [id]="titleId()">{{ entry().title }}</h2>
      @if (!entry().def.requiredChoice) {
        <button type="button" class="close" data-dialog-close data-testid="dialog-close" [attr.aria-label]="t.close" [title]="t.close" (click)="entry().ref.close()">
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
        </button>
      }
    </header>
    <div class="body" #body>
      @if (error(); as e) {
        <p class="failed" role="alert" data-testid="dialog-error">{{ e }}</p>
      } @else if (loading()) {
        <p class="loading" data-testid="dialog-loading">{{ t.loading }}</p>
      }
      <ng-container #content />
    </div>
  `,
  styles: `
    :host { position: fixed; top: 56px; inset-inline-start: 50%; transform: translateX(-50%); max-width: calc(100vw - 32px); max-height: calc(100vh - 96px); box-sizing: border-box; display: flex; flex-direction: column; border-radius: var(--pd-radius-lg, 14px); background: var(--pd-surface-2); color: var(--pd-text); border: 1px solid var(--pd-hairline); box-shadow: var(--pd-elevation-3, 0 12px 32px rgb(0 0 0 / 0.3)); }
    :host(.modeless) { top: 72px; inset-inline-start: auto; inset-inline-end: 24px; transform: none; max-height: calc(100vh - 112px); }
    .head { flex: none; display: flex; align-items: center; justify-content: space-between; gap: var(--pd-space-2, 8px); padding: var(--pd-space-2, 8px) var(--pd-space-2, 8px) var(--pd-space-2, 8px) var(--pd-space-4, 16px); border-bottom: 1px solid var(--pd-hairline); }
    .title { margin: 0; font-size: var(--pd-text-subtitle, 15px); font-weight: var(--pd-weight-semibold, 600); overflow-wrap: anywhere; }
    .close { display: inline-grid; place-items: center; inline-size: 28px; block-size: 28px; border: 0; border-radius: var(--pd-radius-control, 6px); background: transparent; color: var(--pd-text-muted); }
    .close:hover { background: var(--pd-surface-3); color: var(--pd-text); }
    .body { min-height: 0; flex: 1 1 auto; overflow: auto; padding: var(--pd-space-4, 16px); display: flex; flex-direction: column; gap: var(--pd-space-3, 12px); }
    .loading { margin: auto; color: var(--pd-text-muted); }
    .failed { margin: 0; color: var(--pd-danger); overflow-wrap: anywhere; }
  `,
})
export class InPageDialogView {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    close: $localize`:@@pd.dialogs.close:Close`,
    loading: $localize`:@@pd.dialogs.loading:Loading…`,
  };
  readonly entry = input.required<InPageDialog>();
  private readonly environment = inject(EnvironmentInjector);
  private readonly destroyRef = inject(DestroyRef);
  private readonly content = viewChild.required('content', { read: ViewContainerRef });
  private readonly body = viewChild.required<ElementRef<HTMLElement>>('body');
  private readonly element = inject<ElementRef<HTMLElement>>(ElementRef);
  protected readonly loading = signal(true);
  protected readonly error = signal<string | null>(null);
  protected readonly titleId = computed(() => `pd-dialog-title-${this.entry().n}`);
  private readonly size = computed(() => resolveSize(this.entry().def));
  protected readonly width = computed(() => this.size().width);
  /** A tool keeps its height; the other kinds take what their content needs (bounded by the page). */
  protected readonly height = computed(() => (this.entry().def.kind === 'tool' ? this.size().height : undefined));

  constructor() {
    afterNextRender(() => void this.load());
  }

  private async load(): Promise<void> {
    const entry = this.entry();
    try {
      const part = await loadPart(entry.def, this.environment, `dialog ${entry.def.id}`);
      if (part.injector !== this.environment) this.destroyRef.onDestroy(() => part.injector.destroy());
      const injector = contentInjector(part.injector, entry.ref, entry.inputs);
      const ref = mountContent(this.content(), part.component, injector, part.injector, entry.inputs);
      entry.content = ref.instance as DialogContent;
      entry.injector = injector;
      this.loading.set(false);
      queueMicrotask(() => this.focusFirst());
    } catch (e) {
      console.error(`[paddock-dialogs] dialog ${entry.def.id} failed to start`, e);
      this.loading.set(false);
      this.error.set(errorText(e));
    }
  }

  private focusFirst(): void {
    const body = this.body().nativeElement;
    (body.querySelector<HTMLElement>(FOCUSABLE) ?? this.element.nativeElement.querySelector<HTMLElement>(FOCUSABLE))?.focus();
  }

  protected onKey(e: KeyboardEvent): void {
    const action = dialogKeyAction(e);
    if (action === 'dismiss' && e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (!this.entry().def.requiredChoice) this.entry().ref.close();
      return;
    }
    if (action === 'default') {
      const button = findDefaultAction(this.body().nativeElement);
      if (button) {
        e.preventDefault();
        button.click();
      }
      return;
    }
    if (e.key === 'Tab' && this.entry().def.modality === 'modal') this.trapTab(e);
  }

  private trapTab(e: KeyboardEvent): void {
    const items = [...this.element.nativeElement.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = items[0];
    const last = items.at(-1);
    if (!first || !last) return;
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
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
      <div class="scrim" data-testid="dialog-scrim" aria-hidden="true"></div>
    }
    @for (d of stack(); track d.n; let i = $index) {
      @if (d.def.modality === 'modal') {
        <div class="backdrop" data-testid="dialog-backdrop" [style.z-index]="210 + 2 * i" (mousedown)="dismiss(d)"></div>
      }
      <pd-in-page-dialog [entry]="d" [style.z-index]="211 + 2 * i" />
    }
  `,
  styles: `
    .scrim, .backdrop { position: fixed; inset: 0; background: var(--pd-scrim, rgb(0 0 0 / 0.4)); }
    .scrim { z-index: 200; }
  `,
})
export class DialogHost {
  private readonly inPage = inject(InPageDialogBackend);
  private readonly presenter = inject(DialogPresenter, { optional: true });
  private readonly gate = inject(ModalGate);
  protected readonly stack = this.inPage.stack;
  /** A modal dialog window (not one in the page, which has its own backdrop) is open over this window. */
  protected readonly scrim = computed(() => this.presenter?.windowed === true && this.gate.blocked() && !this.stack().some((d) => d.def.modality === 'modal'));
  private restore: HTMLElement | null = null;

  constructor() {
    let depth = 0;
    // A DOM side effect: remembers the focus when the first dialog opens and gives it back when the last one closes.
    effect(() => {
      const count = this.stack().length;
      untracked(() => {
        if (count > 0 && depth === 0) this.restore = document.activeElement as HTMLElement | null;
        if (count === 0 && depth > 0) {
          this.restore?.focus?.();
          this.restore = null;
        }
        depth = count;
      });
    });
  }

  /** Escape from outside any dialog answers the topmost modal (a modeless one answers only from inside it). */
  protected onEscape(e: Event): void {
    if (e.defaultPrevented) return;
    const top = this.stack().filter((d) => d.def.modality === 'modal').at(-1);
    if (!top) return;
    e.preventDefault();
    this.dismiss(top);
  }

  /** Escape and a click on the backdrop mean Cancel; a required choice has none and stays open. */
  protected dismiss(d: InPageDialog): void {
    if (!d.def.requiredChoice) d.ref.close();
  }
}
