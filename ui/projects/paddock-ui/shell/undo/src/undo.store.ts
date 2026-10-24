import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { errorMessage } from '@decryp7/paddock-ui/shell/store';
import { MERGE_WINDOW_MS, UNDO_CLOCK, UNDO_LIMIT, UNDO_TOAST_MS } from './undo-config';

/** The notification kinds the store posts (registered by `UNDO_CONTRIBUTIONS`). */
export const UNDO_NOTICE_KINDS = { failed: 'pd.undo.failed' } as const;

/** One user edit that can be taken back: `do` (re)applies it, `undo` restores what was there before. */
export interface UndoableAction {
  /** Short, lower-case description for "Undo <label>", e.g. "filter change" or "marker rule edit". */
  readonly label: string;
  readonly do: () => void | Promise<void>;
  readonly undo: () => void | Promise<void>;
  /** Consecutive actions with the same key within `MERGE_WINDOW_MS` are one entry (the first `undo`, the last `do`). */
  readonly mergeKey?: string;
  /** Entries of a group can be dropped together with `discard` (the edits of a draft that was saved or cancelled). */
  readonly group?: string;
  /** Show the "Done, Undo" toast (for deliberate actions; not for typing or dragging). */
  readonly announce?: boolean;
}

interface Entry {
  readonly action: UndoableAction;
  readonly at: number;
}

interface UndoState {
  past: readonly Entry[];
  future: readonly Entry[];
  running: boolean;
  /** The toast being shown; `seq` tells a newer toast from the one a timer was started for. */
  toast: { readonly label: string; readonly seq: number } | null;
  seq: number;
}

const capitalise = (s: string): string => s.charAt(0).toUpperCase() + s.slice(1);

/**
 * Undo and redo for the user's own edits (the port of Debrief's `UndoStack`; plan X44). Features register what they did
 * (`record`, or `execute` to do and record at once); undo and redo replay the closures one at a time. Edits that change
 * the engine undo by re-applying the previous state through the normal validated path. A failure is a notice, never a
 * throw, and leaves the history where it was. Provided per window by `provideShellUndo`.
 */
export const UndoStore = signalStore(
  withState<UndoState>({ past: [], future: [], running: false, toast: null, seq: 0 }),
  withComputed((store) => ({
    canUndo: computed(() => store.past().length > 0 && !store.running()),
    canRedo: computed(() => store.future().length > 0 && !store.running()),
    undoLabel: computed(() => store.past().at(-1)?.action.label ?? null),
    redoLabel: computed(() => store.future().at(-1)?.action.label ?? null),
    /** True while an undo or redo runs: observers must not record the changes it makes. */
    applying: computed(() => store.running()),
    size: computed(() => store.past().length),
  })),
  withMethods((store, ports = inject(SHELL_PORTS), clock = inject(UNDO_CLOCK)) => {
    let cancelToast: (() => void) | null = null;

    const hideToast = (): void => {
      cancelToast?.();
      cancelToast = null;
      if (store.toast()) patchState(store, { toast: null });
    };

    const showToast = (label: string): void => {
      cancelToast?.();
      const seq = store.seq() + 1;
      patchState(store, { toast: { label, seq }, seq });
      cancelToast = clock.after(UNDO_TOAST_MS, () => {
        if (store.toast()?.seq === seq) hideToast();
      });
    };

    const fail = (text: string): void => ports.notify({ level: 'error', kind: UNDO_NOTICE_KINDS.failed, message: text });

    const push = (action: UndoableAction): void => {
      const at = clock.now();
      const last = store.past().at(-1);
      if (action.mergeKey && last?.action.mergeKey === action.mergeKey && at - last.at <= MERGE_WINDOW_MS) {
        patchState(store, (s) => ({ past: [...s.past.slice(0, -1), { action: { ...action, undo: last.action.undo }, at }], future: [] }));
      } else {
        patchState(store, (s) => ({ past: [...s.past, { action, at }].slice(-UNDO_LIMIT), future: [] }));
      }
      if (action.announce) showToast(action.label);
    };

    const step = async (op: 'undo' | 'redo'): Promise<boolean> => {
      // Everything is read before the first await (rule 29).
      const entry = (op === 'undo' ? store.past() : store.future()).at(-1);
      if (!entry || store.running()) return false;
      patchState(store, { running: true });
      hideToast();
      try {
        await (op === 'undo' ? entry.action.undo() : entry.action.do());
        patchState(store, (s) =>
          op === 'undo'
            ? { past: s.past.slice(0, -1), future: [...s.future, { ...entry, action: { ...entry.action, mergeKey: undefined } }].slice(-UNDO_LIMIT) }
            : { future: s.future.slice(0, -1), past: [...s.past, { ...entry, action: { ...entry.action, mergeKey: undefined } }].slice(-UNDO_LIMIT) },
        );
        return true;
      } catch (e) {
        const label = entry.action.label;
        const cause = errorMessage(e, $localize`:@@pd.undo.unknown:something went wrong`);
        fail(
          op === 'undo'
            ? $localize`:@@pd.undo.undoFailed:Could not undo ${label}:label:: ${cause}:cause:`
            : $localize`:@@pd.undo.redoFailed:Could not redo ${label}:label:: ${cause}:cause:`,
        );
        return false;
      } finally {
        patchState(store, { running: false });
      }
    };

    return {
      /** Records an edit that has already been made. Ignored while an undo or redo is being applied. */
      record(action: UndoableAction): void {
        if (!store.running()) push(action);
      },

      /** Makes an edit (`do`) and records it. Resolves false (and records nothing) when `do` fails. */
      async execute(action: UndoableAction): Promise<boolean> {
        try {
          await action.do();
        } catch (e) {
          const label = capitalise(action.label);
          const cause = errorMessage(e, $localize`:@@pd.undo.unknown:something went wrong`);
          fail($localize`:@@pd.undo.actionFailed:${label}:label: failed: ${cause}:cause:`);
          return false;
        }
        if (!store.running()) push(action);
        return true;
      },

      /** Takes back the latest edit. Resolves false when there is none, another one runs, or it failed. */
      undo: () => step('undo'),

      /** Re-applies the latest undone edit. */
      redo: () => step('redo'),

      /** Forgets everything (another project or workspace replaces what the edits referred to). */
      clear(): void {
        hideToast();
        patchState(store, { past: [], future: [] });
      },

      /** Forgets the entries of one group (their state no longer exists), keeping the others in order. */
      discard(group: string): void {
        patchState(store, (s) => ({
          past: s.past.filter((e) => e.action.group !== group),
          future: s.future.filter((e) => e.action.group !== group),
        }));
      },

      /** Hides the toast (its close button; the history is untouched). */
      dismissToast: hideToast,
    };
  }),
);

export type UndoStoreType = InstanceType<typeof UndoStore>;
