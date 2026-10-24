import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, untracked } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
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
  imports: [PdGlyph, PdViewNode],
  providers: [ViewStore, ViewRuntime],
  host: { '[attr.aria-busy]': 'busy() || store.isLoading() ? "true" : null' },
  template: `
    @if (store.requestError(); as message) {
      <div class="pd-view-error" role="alert" data-testid="view-error" [attr.data-kind]="store.isStale() ? 'stale' : 'failed'">
        <pd-glyph name="alert" [size]="16" />
        <div class="pd-view-error__body">
          <p class="pd-view-error__title">{{ store.isStale() ? staleTitle : errorTitle }}</p>
          <p class="pd-view-error__message">{{ message }}</p>
        </div>
        <button type="button" class="pd-btn" data-testid="view-retry" (click)="retry.emit()">{{ retryText }}</button>
      </div>
    }
    @if (runtime.error(); as problem) {
      <p class="pd-view-notice" role="alert" data-testid="view-notice"><pd-glyph name="alert" [size]="14" />{{ problem }}</p>
    }
    @if (store.doc(); as d) {
      <article class="pd-view" [attr.aria-label]="d.title">
        @if (showTitle()) {
          <h2 class="pd-view__title" data-testid="view-title">{{ d.title }}</h2>
        }
        <pd-view-node [node]="d.root" />
      </article>
    } @else if (store.isLoading()) {
      <p class="pd-view-loading" role="status" data-testid="view-loading">{{ loadingText }}</p>
    }
  `,
  styles: `
    :host { display: block; container-type: inline-size; min-width: 0; color: var(--pd-text); }
    .pd-view { display: grid; gap: var(--pd-space-3); min-width: 0; }
    .pd-view__title { margin: 0; font-size: var(--pd-text-title); line-height: var(--pd-lh-title); font-weight: var(--pd-weight-semibold); }
    .pd-view-error { display: flex; align-items: flex-start; gap: var(--pd-space-3); padding: var(--pd-space-3); margin-bottom: var(--pd-space-3); border: 1px solid var(--pd-danger); border-radius: var(--pd-radius-card); background: var(--pd-danger-tint); color: var(--pd-text); flex-wrap: wrap; }
    .pd-view-error__body { flex: 1 1 14rem; min-width: 0; }
    .pd-view-error__title { margin: 0; font-weight: var(--pd-weight-semibold); }
    .pd-view-error__message { margin: 0; overflow-wrap: anywhere; color: var(--pd-text-body); }
    .pd-view-notice { display: flex; align-items: center; gap: var(--pd-space-2); margin: 0 0 var(--pd-space-3); color: var(--pd-danger); }
    .pd-view-loading { margin: 0; color: var(--pd-text-muted); }
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
  /** Retry on the inline error: ask the engine for the view again. */
  readonly retry = output<void>();

  protected readonly errorTitle = $localize`:@@pd.view.error:This view cannot be shown`;
  protected readonly staleTitle = $localize`:@@pd.view.stale:This view is out of date`;
  protected readonly retryText = $localize`:@@pd.view.retry:Retry`;
  protected readonly loadingText = $localize`:@@pd.view.loading:Loading…`;
  private readonly title = computed(() => this.store.doc()?.title ?? '');

  constructor() {
    this.runtime.attach({ view: this.view, title: this.title, busy: this.busy, emit: (a) => this.action.emit(a) });
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
