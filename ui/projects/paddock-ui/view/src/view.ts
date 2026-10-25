import { booleanAttribute, ChangeDetectionStrategy, Component, computed, effect, inject, input, output, untracked } from '@angular/core';
import { PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import type { ViewAction } from '../generated/view';
import { PdViewNode } from './node';
import type { PatchResult } from './patch';
import { ViewRuntime } from './runtime';
import { ViewStore } from './view.store';

/**
 * Renders a `ViewDoc` that a plugin returned (ADR 0002; `paddock-view` in Rust): pure data validated against the limits on receipt, drawn
 * by the family's components so every app looks the same. Give it the `view` id (the contribution it belongs to) and the received JSON
 * as `doc`; it emits a `ViewAction` for everything the user does (`(action)`), and the host answers with a patch
 * (`applyPatch`, a replace-by-id with the base etag checked, or a whole new document) or a new `doc`. An invalid document or a stale
 * patch shows an inline error with Retry (`(retry)`: ask the engine for the view again); the last good document stays on screen
 * when only a patch was refused. Confirmations, the OS browser and the path and secret pickers go through `VIEW_HOST`.
 *
 * ```html
 * <pd-view view="pacenotes.review" [doc]="doc()" [busy]="working()" (action)="send($event)" (retry)="reload()" #v />
 * <!-- when the engine answers: v.applyPatch(patch) -->
 * ```
 */
@Component({
  selector: 'pd-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdStatusGlyph, PdViewNode],
  providers: [ViewStore, ViewRuntime],
  host: { '[attr.aria-busy]': 'busy() || store.isLoading() ? "true" : null' },
  template: `
    @if (store.requestError(); as message) {
      <div class="pd-view-error" [class.pd-view-error--stale]="store.isStale()" role="alert" data-testid="view-error" [attr.data-kind]="store.isStale() ? 'stale' : 'failed'">
        <pd-status-glyph class="pd-view-error__icon" status="error" [size]="16" />
        <div class="pd-view-error__body">
          <p class="pd-view-error__title">{{ store.isStale() ? staleTitle : failedTitle() }}</p>
          <p class="pd-view-error__message">{{ message }}</p>
        </div>
        <div class="pd-view-error__actions">
          <button type="button" class="pd-btn" data-testid="view-retry" (click)="retry.emit()">{{ store.isStale() ? reloadText : retryText }}</button>
          @if (logsLink() && !store.isStale()) {
            <button type="button" class="pd-btn pd-btn--ghost" data-testid="view-logs" (click)="openLogs.emit()">{{ logsText }}</button>
          }
        </div>
      </div>
    }
    @if (runtime.error(); as problem) {
      <p class="pd-view-notice" role="alert" data-testid="view-notice"><pd-status-glyph status="error" [size]="14" />{{ problem }}</p>
    }
    @if (busy()) {
      <div class="pd-view-busy" role="status" data-testid="view-busy">
        <span class="pd-view-busy__bar" aria-hidden="true"></span>
        <span class="pd-view-busy__text">{{ workingText }}</span>
      </div>
    }
    @if (store.doc(); as d) {
      <article class="pd-view" [attr.aria-label]="d.title">
        @if (showTitle()) {
          <h2 class="pd-view__title" data-testid="view-title">{{ d.title }}</h2>
        }
        <pd-view-node [node]="d.root" />
      </article>
    } @else if (store.isLoading()) {
      <div class="pd-view pd-view-skeleton" role="status" data-testid="view-loading">
        <span class="pd-view-skeleton__bar pd-view-skeleton__bar--title"></span>
        <span class="pd-view-skeleton__bar"></span>
        <span class="pd-view-skeleton__bar pd-view-skeleton__bar--short"></span>
        <span class="pd-view-skeleton__block"></span>
        <span class="pd-view-sr">{{ loadingText }}</span>
      </div>
    }
  `,
  styles: `
    :host { display: block; container-type: inline-size; min-width: 0; color: var(--pd-text); }
    .pd-view { display: grid; gap: var(--pd-space-3); min-width: 0; }
    .pd-view__title { margin: 0; font-size: var(--pd-text-title); line-height: var(--pd-lh-title); font-weight: var(--pd-weight-semibold); }
    .pd-view-error { display: flex; align-items: flex-start; gap: var(--pd-space-3); padding: var(--pd-space-3); margin-bottom: var(--pd-space-3); border: 1px solid var(--pd-danger); border-radius: var(--pd-radius-card); background: var(--pd-danger-tint); color: var(--pd-text); flex-wrap: wrap; }
    .pd-view-error__icon { color: var(--pd-danger); margin-top: 2px; }
    /* Stale is not a failure: the last good view is still on screen, so it is a warning, not an error. */
    .pd-view-error--stale { border-color: var(--pd-warning); background: var(--pd-warning-tint); }
    .pd-view-error--stale .pd-view-error__icon { color: var(--pd-warning); }
    .pd-view-error__body { flex: 1 1 14rem; min-width: 0; }
    .pd-view-error__actions { display: flex; gap: var(--pd-space-2); flex-wrap: wrap; }
    .pd-view-error__title { margin: 0; font-weight: var(--pd-weight-semibold); }
    .pd-view-error__message { margin: 0; overflow-wrap: break-word; color: var(--pd-text-body); }
    .pd-view-notice { display: flex; align-items: center; gap: var(--pd-space-2); margin: 0 0 var(--pd-space-3); color: var(--pd-danger); }
    .pd-view-busy { position: relative; display: flex; justify-content: flex-end; align-items: center; min-height: 20px; margin-bottom: var(--pd-space-2); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-view-busy__bar { position: absolute; inset: 0 0 auto 0; height: 2px; overflow: hidden; background: var(--pd-surface-3); border-radius: 1px; }
    .pd-view-busy__bar::after { content: ''; position: absolute; inset: 0 auto 0 0; width: 30%; background: var(--pd-accent); border-radius: inherit; animation: pd-view-busy calc(1.4s * var(--pd-motion-scale, 1)) var(--pd-ease-move) infinite; }
    .pd-view-busy__text { padding-top: var(--pd-space-2); }
    @keyframes pd-view-busy { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
    @media (prefers-reduced-motion: reduce) { .pd-view-busy__bar::after { animation: none; width: 100%; opacity: 0.5; } }
    .pd-view-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    /* The loading skeleton has the geometry of a view: a title line, two text lines and a block. */
    .pd-view-skeleton__bar { display: block; height: 14px; border-radius: var(--pd-radius-tag); background: var(--pd-skeleton); }
    .pd-view-skeleton__bar--title { height: 22px; width: 40%; }
    .pd-view-skeleton__bar--short { width: 65%; }
    .pd-view-skeleton__block { display: block; height: 112px; border-radius: var(--pd-radius-card); background: var(--pd-skeleton); }
    .pd-view-skeleton > span { background-image: linear-gradient(90deg, transparent, var(--pd-skeleton-shine), transparent); background-size: 200% 100%; background-repeat: no-repeat; animation: pd-view-shine calc(1.6s * var(--pd-motion-scale, 1)) linear infinite; }
    .pd-view-skeleton > .pd-view-sr { animation: none; background: none; }
    @keyframes pd-view-shine { from { background-position: 200% 0; } to { background-position: -100% 0; } }
    @media (prefers-reduced-motion: reduce) { .pd-view-skeleton > span { animation: none; background-image: none; } }
    @container (max-width: 480px) { .pd-view { gap: var(--pd-space-2); } }
  `,
})
export class PdView {
  protected readonly store = inject(ViewStore);
  protected readonly runtime = inject(ViewRuntime);
  /** The view's id (the contribution it belongs to); it is the `view` of every `ViewAction`. */
  readonly view = input.required<string>();
  /** The document as received (JSON from the engine). It is validated; `null` shows nothing (or loading). */
  readonly doc = input<unknown>(null);
  /** The host is waiting for the engine's answer to an action: the controls are off. */
  readonly busy = input(false);
  /** The host is fetching the view: a "Loading" line while there is no document yet. */
  readonly loading = input(false);
  /** Draws the document's title above it (turn off when the pane has its own header). */
  readonly showTitle = input(true);
  /** What the user did, for the engine. */
  readonly action = output<ViewAction>();
  /** Retry on the inline error (Reload for a stale view): ask the engine for the view again. */
  readonly retry = output<void>();
  /** The plugin the view comes from, as the user knows it: an invalid document is then "<plugin> sent a view this app cannot show". */
  readonly plugin = input('');
  /** Offer "Open logs" beside Retry on an invalid view (the app's log page). */
  readonly logsLink = input(false, { transform: booleanAttribute });
  /** The user chose "Open logs" on the inline error. */
  readonly openLogs = output<void>();

  protected readonly errorTitle = $localize`:@@pd.view.error:This view cannot be shown`;
  protected readonly reloadText = $localize`:@@pd.view.reload:Reload`;
  protected readonly logsText = $localize`:@@pd.view.logs:Open logs`;
  protected readonly workingText = $localize`:@@pd.view.working:Working…`;
  /** The title of a failed view: names the plugin when the host says which it is. */
  protected readonly failedTitle = computed(() => (this.plugin() ? $localize`:@@pd.view.error-plugin:${this.plugin()}:plugin: sent a view this app cannot show` : this.errorTitle));
  protected readonly staleTitle = $localize`:@@pd.view.stale:This view is out of date`;
  protected readonly retryText = $localize`:@@pd.view.retry:Retry`;
  protected readonly loadingText = $localize`:@@pd.view.loading:Loading…`;
  private readonly title = computed(() => this.store.doc()?.title ?? '');

  constructor() {
    this.runtime.attach({ view: this.view, title: this.title, root: computed(() => this.store.doc()?.root ?? null), busy: this.busy, emit: (a) => this.action.emit(a) });
    // A side effect on the store (outside the signal graph's pure derivations): a document or a loading flag from the host replaces what the store holds.
    effect(() => {
      const raw = this.doc();
      const loading = this.loading();
      untracked(() => {
        if (raw !== null && raw !== undefined) this.store.show(raw);
        else if (loading) this.store.begin();
        else this.store.reset();
      });
    });
  }

  /** Applies the engine's answer to an action: a `ViewPatch` (`{ replace: { baseEtag, etag, id, node } }` or `{ doc }`). A refusal shows the inline error with Retry and keeps the document. */
  applyPatch(patch: unknown): PatchResult {
    return this.store.applyPatch(patch);
  }
}
