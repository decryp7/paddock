import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { errorMessage, injectTrackLoad, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { PreviewReply, RedactionCounts, Saved, SaveOutcome } from '../generated/diagnostics';
import { DiagnosticsBackend, type Unlisten } from './backend';
import { asDiagError, diagMessage } from './wire';

/** Where the save is. Never two of these at once: one union, not separate flags. */
export type SaveState =
  | { readonly kind: 'idle' }
  /** The native dialog is open, or the zip is being written (the engine's job shows the progress). */
  | { readonly kind: 'saving' }
  | { readonly kind: 'saved'; readonly saved: Saved }
  /** The user closed the save dialog: nothing was written. */
  | { readonly kind: 'dismissed' }
  /** The user cancelled: nothing was written. */
  | { readonly kind: 'cancelled' }
  | { readonly kind: 'failed'; readonly message: string };

interface DiagnosticsState {
  /** What the engine collected: the files with their sizes and the token a save refers to. */
  reply: PreviewReply | null;
  /** What the user typed. */
  description: string;
  saveState: SaveState;
}

/** What redaction removed, in the order shown, in words ("2 secrets", "1 user name"); kinds with a zero count are left out. */
export function redactionSummary(c: RedactionCounts): string[] {
  const rows: readonly [number, (n: number) => string][] = [
    [c.secrets, (n) => (n === 1 ? $localize`:@@pd.diag.removed.secret:1 secret` : $localize`:@@pd.diag.removed.secrets:${n}:n: secrets`)],
    [c.fields, (n) => (n === 1 ? $localize`:@@pd.diag.removed.field:1 hidden value` : $localize`:@@pd.diag.removed.fields:${n}:n: hidden values`)],
    [c.userNames, (n) => (n === 1 ? $localize`:@@pd.diag.removed.user:1 user name` : $localize`:@@pd.diag.removed.users:${n}:n: user names`)],
    [c.homePaths + c.profilePaths, (n) => (n === 1 ? $localize`:@@pd.diag.removed.home:1 home folder` : $localize`:@@pd.diag.removed.homes:${n}:n: home folders`)],
    [c.machineNames, (n) => (n === 1 ? $localize`:@@pd.diag.removed.machine:1 computer name` : $localize`:@@pd.diag.removed.machines:${n}:n: computer names`)],
    [c.emails, (n) => (n === 1 ? $localize`:@@pd.diag.removed.email:1 e-mail address` : $localize`:@@pd.diag.removed.emails:${n}:n: e-mail addresses`)],
    [c.ipAddresses, (n) => (n === 1 ? $localize`:@@pd.diag.removed.ip:1 IP address` : $localize`:@@pd.diag.removed.ips:${n}:n: IP addresses`)],
  ];
  return rows.filter(([n]) => n > 0).map(([n, words]) => words(n));
}

/**
 * Report a problem as one window sees it (rule 34: the engine is the single source of truth). The engine collects and
 * redacts; the store holds the preview it returned, the description the user types, and the save. Every command returns at
 * once; the zip is written by a job in the engine and its outcome arrives as an event. Nothing here times out a save
 * (rule 12) or supplies a path: the user chooses it in the native dialog.
 */
export const DiagnosticsStore = signalStore(
  withState<DiagnosticsState>({ reply: null, description: '', saveState: { kind: 'idle' } }),
  withRequestStatus(),
  withComputed(({ reply, saveState }) => ({
    files: computed(() => reply()?.preview.files ?? []),
    notes: computed(() => reply()?.preview.notes ?? []),
    totalBytes: computed(() => reply()?.preview.totalBytes ?? 0),
    removed: computed(() => (reply() ? redactionSummary(reply()!.preview.redactions) : [])),
    saving: computed(() => saveState().kind === 'saving'),
  })),
  withMethods((store, backend = inject(DiagnosticsBackend)) => {
    const load = async (token: string | null): Promise<boolean> => {
      // Reads before the first await (rule 29).
      const description = store.description();
      // A refresh keeps the list on screen; only the first collection shows as loading.
      if (!store.reply()) patchState(store, setLoading());
      try {
        const reply = await backend.preview(token, description);
        patchState(store, { reply }, setLoaded());
        return true;
      } catch (e) {
        patchState(store, setError(diagMessage(e, $localize`:@@pd.diag.no-preview:The report could not be put together.`)));
        return false;
      }
    };
    return {
      /** Collects the report (also the Retry of a failed one, and the refresh after the engine says the preview is out of date). */
      load: () => load(null),
      setDescription(description: string): void {
        patchState(store, { description });
      },
      /** The description is final for now (focus left the box): the engine re-assembles the report with it, without collecting again. */
      async commitDescription(): Promise<void> {
        await load(store.reply()?.token ?? null);
      },
      /** Asks the engine to save: it asks the user where, then writes; the outcome follows as an event. */
      async save(): Promise<void> {
        const reply = store.reply();
        const description = store.description();
        if (!reply || store.saveState().kind === 'saving') return;
        patchState(store, { saveState: { kind: 'saving' } });
        try {
          await backend.save(reply.token, description);
        } catch (e) {
          const code = asDiagError(e)?.code;
          if (code === 'expired') {
            // The engine dropped that preview: show the new one, and say why nothing was saved.
            await load(null);
          }
          patchState(store, { saveState: { kind: 'failed', message: diagMessage(e, $localize`:@@pd.diag.save-refused:The report could not be saved.`) } });
        }
      },
      /** Stops the save in progress; nothing is written. */
      async cancel(): Promise<void> {
        try {
          await backend.cancel();
        } catch (e) {
          patchState(store, { saveState: { kind: 'failed', message: diagMessage(e, $localize`:@@pd.diag.cancel-refused:The save could not be cancelled.`) } });
        }
      },
      /** An outcome from the engine's event. */
      _outcome(o: SaveOutcome): void {
        switch (o.state) {
          case 'saved': {
            const { state: _state, ...saved } = o;
            patchState(store, { saveState: { kind: 'saved', saved } });
            return;
          }
          case 'failed':
            patchState(store, { saveState: { kind: 'failed', message: o.message } });
            return;
          default:
            patchState(store, { saveState: { kind: o.state } });
        }
      },
      /** Back to the preview after a dismissed, cancelled or failed save. */
      acknowledge(): void {
        patchState(store, { saveState: { kind: 'idle' } });
      },
    };
  }),
  withHooks((store, backend = inject(DiagnosticsBackend), trackLoad = injectTrackLoad()) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then collect: an outcome can only follow a save, which needs the preview.
        void trackLoad(() =>
          Promise.all([
            backend
              .listen((o) => store._outcome(o))
              .then((u) => (destroyed ? u() : (off = u)))
              .catch((e: unknown) => console.warn(`[paddock-diagnostics] cannot listen for save outcomes: ${errorMessage(e, 'unknown error')}`)),
            store.load(),
          ]),
        );
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type DiagnosticsStoreType = InstanceType<typeof DiagnosticsStore>;
