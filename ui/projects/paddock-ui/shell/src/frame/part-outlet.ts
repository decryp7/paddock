import {
  ChangeDetectionStrategy,
  Component,
  ComponentRef,
  computed,
  DestroyRef,
  effect,
  EnvironmentInjector,
  inject,
  input,
  signal,
  untracked,
  ViewContainerRef,
  viewChild,
} from '@angular/core';
import { SuErrorState, SuSkeleton } from '@decryp7/paddock-ui';
import { describe } from '../context';
import { loadPart, type LazyPart } from '../lazy';

/** Where a part is loading, shown, or failed to load (with the reason). */
export type PartStatus = { readonly kind: 'loading' } | { readonly kind: 'loaded' } | { readonly kind: 'error'; readonly reason: string };

/**
 * Renders a lazy part (a page, a status item): loads its component and providers with `loadPart`, creates the component
 * in an injector of its own (destroyed with it, rule 32), and contains a failure to the part (an error with Retry instead
 * of a broken window; AGENTS "each UI panel has an error boundary"). `variant` sets how the states look: a page has a
 * skeleton and a full error state, an inline part (status item) nothing while it loads and a small Retry button.
 */
@Component({
  selector: 'pd-part-outlet',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuErrorState, SuSkeleton],
  host: { '[attr.data-part-status]': 'status().kind', '[class.pd-page]': 'variant() === "page"' },
  template: `
    <ng-container #slot />
    @switch (status().kind) {
      @case ('loading') {
        @if (variant() === 'page') {
          <div class="pd-loading" role="status" [attr.aria-label]="loadingLabel()" data-testid="part-loading">
            <pd-skeleton [height]="26" width="40%" />
            <pd-skeleton [height]="14" width="70%" />
            <pd-skeleton shape="block" [height]="160" />
          </div>
        }
      }
      @case ('error') {
        @if (variant() === 'page') {
          <pd-error-state [heading]="openFailed()" [cause]="reason()" [retryLabel]="t.tryAgain" (retry)="load()" data-testid="part-error" />
        } @else {
          <button type="button" class="pd-retry" [attr.aria-label]="retryLabel()" [title]="failedTitle()" data-testid="part-error" (click)="load()">
            <span aria-hidden="true">!</span>
          </button>
        }
      }
    }
  `,
  styles: `
    :host { display: contents; }
    /* A routed page's box (ADR 0012): a flex item of pd-page-host's column that may shrink to the host, so the page's own
       height: 100% resolves. A page with height: 100% and overflow: hidden is clipped to the host, not scrolled. */
    :host(.pd-page) { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; min-width: 0; }
    .pd-loading { display: flex; flex-direction: column; gap: var(--pd-space-3); padding: var(--pd-space-6); max-width: var(--pd-content-max); }
    .pd-retry { display: inline-flex; align-items: center; justify-content: center; width: 20px; height: 20px; padding: 0; border: 1px solid var(--pd-danger); border-radius: var(--pd-radius-pill); background: transparent; color: var(--pd-danger); font: inherit; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-semibold); cursor: pointer; }
    .pd-retry:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  `,
})
export class PdPartOutlet {
  /** The part to render. A new part replaces the old one (its component and injector are destroyed). */
  readonly part = input.required<LazyPart>();
  /** What the part is called, for the loading and error states ("Models"). */
  readonly label = input.required<string>();
  protected readonly t = { tryAgain: $localize`:@@pd.shell.part.try-again:Try again` };
  protected readonly loadingLabel = computed(() => $localize`:@@pd.shell.part.loading:Loading ${this.label()}:part:`);
  protected readonly openFailed = computed(() => $localize`:@@pd.shell.part.open-failed:${this.label()}:part: could not be opened.`);
  protected readonly retryLabel = computed(() => $localize`:@@pd.shell.part.retry:${this.label()}:part: did not load. Try again`);
  protected readonly failedTitle = computed(() => $localize`:@@pd.shell.part.failed:${this.label()}:part: did not load: ${this.reason()}:reason:`);
  readonly variant = input<'page' | 'inline'>('inline');

  private readonly parent = inject(EnvironmentInjector);
  private readonly slot = viewChild.required('slot', { read: ViewContainerRef });
  private readonly state = signal<PartStatus>({ kind: 'loading' });
  readonly status = this.state.asReadonly();
  private ref: ComponentRef<unknown> | null = null;
  private injector: EnvironmentInjector | null = null;
  /** Bumped by every load: a load that finishes after a newer one started is dropped. */
  private generation = 0;

  constructor() {
    // Loading the part is a side effect (an import and a component outside the signal graph); it reads only `part`.
    effect(() => {
      const part = this.part();
      untracked(() => this.load(part));
    });
    inject(DestroyRef).onDestroy(() => {
      this.generation++;
      this.clear();
    });
  }

  protected reason(): string {
    const s = this.state();
    return s.kind === 'error' ? s.reason : '';
  }

  /** Loads (again) and shows the part; a failure becomes the error state. */
  async load(part: LazyPart = this.part()): Promise<void> {
    const generation = ++this.generation;
    const label = this.label();
    const slot = this.slot();
    this.clear();
    this.state.set({ kind: 'loading' });
    try {
      const loaded = await loadPart(part, this.parent, `part ${label}`);
      if (generation !== this.generation) {
        if (loaded.injector !== this.parent) loaded.injector.destroy();
        return;
      }
      this.injector = loaded.injector === this.parent ? null : loaded.injector;
      this.ref = slot.createComponent(loaded.component, { environmentInjector: loaded.injector });
      this.state.set({ kind: 'loaded' });
    } catch (e) {
      if (generation !== this.generation) return;
      this.clear();
      console.error(`[paddock-shell] ${label} failed to load`, e);
      this.state.set({ kind: 'error', reason: describe(e) });
    }
  }

  private clear(): void {
    this.ref?.destroy();
    this.ref = null;
    this.injector?.destroy();
    this.injector = null;
  }
}
