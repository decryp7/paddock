import { computed } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import { setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { ViewDoc } from '../generated/view';
import { validateViewDoc, type ViewIssue } from './limits';
import { applyViewPatch, type PatchProblem, type PatchResult } from './patch';

/** Why the view shows an error: the document broke the limits, or a patch could not be applied. */
export type ViewFailure = { readonly kind: 'invalid'; readonly issue: ViewIssue } | { readonly kind: 'patch'; readonly problem: PatchProblem };

interface ViewState {
  /** The document on screen (valid); null before the first one, or when the last one received was not valid. */
  doc: ViewDoc | null;
  /** What went wrong last; null when nothing did. The message is also in `requestStatus` (rule 30). */
  failure: ViewFailure | null;
}

/**
 * The document one `<pd-view>` shows (ADR 0002): receives a document (validated against the limits on receipt, so an invalid one
 * is an inline error with Retry and is never drawn), applies `ViewPatch`es to it (replace a node by id with the base etag
 * checked, or a whole new document) and keeps the last good document when a patch is refused. A stale patch is an error state
 * too: the host asks the engine for the view again. Provide it with the component (`PdView` does); change it only through these methods.
 */
export const ViewStore = signalStore(
  withState<ViewState>({ doc: null, failure: null }),
  withRequestStatus(),
  withComputed(({ failure }) => ({
    /** The refusal is a stale patch: the engine's view moved on, so reload rather than retry the same patch. */
    isStale: computed(() => {
      const f = failure();
      return f?.kind === 'patch' && f.problem.kind === 'stale';
    }),
  })),
  withMethods((store) => ({
    /** The host is fetching the view: the document on screen stays, the status says loading. */
    begin(): void {
      patchState(store, setLoading());
    },
    /** Shows a received document, or the reason it is not valid. A new document replaces the old one even when it is invalid. */
    show(raw: unknown): void {
      const issue = validateViewDoc(raw);
      if (issue) patchState(store, { doc: null, failure: { kind: 'invalid', issue } }, setError(issue.message));
      else patchState(store, { doc: raw as ViewDoc, failure: null }, setLoaded());
    },
    /** Applies a patch; on a refusal the document on screen is unchanged and the error says why. */
    applyPatch(raw: unknown): PatchResult {
      const doc = store.doc();
      const result: PatchResult = doc
        ? applyViewPatch(doc, raw)
        : { ok: false, problem: { kind: 'malformed', message: $localize`:@@pd.view.patch.no-doc:There is no view to update yet.` } };
      if (result.ok) patchState(store, { doc: result.doc, failure: null }, setLoaded());
      else patchState(store, { failure: { kind: 'patch', problem: result.problem } }, setError(result.problem.message));
      return result;
    },
    /** Forgets the document and the error (the view was closed or has no document yet). */
    reset(): void {
      patchState(store, { doc: null, failure: null, requestStatus: 'idle' });
    },
  })),
);
export type ViewStoreType = InstanceType<typeof ViewStore>;
