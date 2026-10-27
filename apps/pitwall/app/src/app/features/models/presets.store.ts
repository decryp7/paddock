import { inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import { computed } from '@angular/core';
import type { CommandError, PresetDraft, PresetEntry, PresetPreview } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';

/** How long typing pauses before the engine is asked what the draft resolves to. */
export const PREVIEW_DELAY_MS = 250;

interface PresetsState {
  entries: PresetEntry[];
  loading: boolean;
  loaded: boolean;
  error: CommandError | null;
  /** The editor: `editing` is the alias being edited, null for a new preset. Null when closed. */
  editor: { editing: string | null } | null;
  preview: PresetPreview | null;
  previewing: boolean;
  saving: boolean;
  saveError: CommandError | null;
  /** Aliases being deleted. */
  deleting: string[];
}

const initial: PresetsState = { entries: [], loading: false, loaded: false, error: null, editor: null, preview: null, previewing: false, saving: false, saveError: null, deleting: [] };

/** Model presets and aliases: the list with where each stands now, the editor, and the engine's live check of a draft. */
export const PresetsStore = signalStore(
  withState(initial),
  withComputed(({ entries }) => ({
    /** Presets that cannot be served now (their model was removed, nothing matches, or a model shadows them). */
    broken: computed(() => entries().filter((e) => e.status.state !== 'ready').length),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let seq = 0;
    const cancelPreview = () => {
      if (timer) clearTimeout(timer);
      timer = null;
      seq++;
    };

    async function load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        patchState(store, { entries: await backend.presets(), loading: false, loaded: true });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    }

    async function previewNow(draft: PresetDraft): Promise<void> {
      const mine = ++seq;
      patchState(store, { previewing: true });
      try {
        const preview = await backend.resolvePreset(draft, store.editor()?.editing ?? null);
        if (mine === seq) patchState(store, { preview, previewing: false });
      } catch (e) {
        const err = toCommandError(e);
        if (mine === seq) patchState(store, { preview: { problem: err.message, status: null }, previewing: false });
      }
    }

    return {
      load,

      /** Opens the editor on a new preset, or on the one with this alias. */
      openEditor(editing: string | null): void {
        cancelPreview();
        patchState(store, { editor: { editing }, preview: null, previewing: false, saveError: null });
      },

      closeEditor(): void {
        cancelPreview();
        patchState(store, { editor: null, preview: null, previewing: false, saveError: null });
      },

      /** Asks the engine what a draft resolves to, after typing pauses. */
      schedulePreview(draft: PresetDraft): void {
        if (timer) clearTimeout(timer);
        seq++;
        timer = setTimeout(() => {
          timer = null;
          void previewNow(draft);
        }, PREVIEW_DELAY_MS);
      },

      /** Saves the draft (a new preset, or the edited one). The editor closes on success. */
      async save(draft: PresetDraft): Promise<boolean> {
        const editor = store.editor();
        if (!editor || store.saving()) return false;
        cancelPreview();
        patchState(store, { saving: true, saveError: null });
        try {
          const saved = editor.editing === null ? await backend.createPreset(draft) : await backend.updatePreset(editor.editing, draft);
          patchState(store, { saving: false, editor: null, preview: null });
          toasts.ok(editor.editing === null ? `Created ${saved.alias}.` : `Saved ${saved.alias}.`);
          await load();
          return true;
        } catch (e) {
          patchState(store, { saving: false, saveError: toCommandError(e) });
          return false;
        }
      },

      async remove(alias: string): Promise<boolean> {
        if (store.deleting().includes(alias)) return false;
        patchState(store, { deleting: [...store.deleting(), alias] });
        try {
          await backend.deletePreset(alias);
          toasts.ok(`Deleted ${alias}.`);
          if (store.editor()?.editing === alias) patchState(store, { editor: null, preview: null });
          await load();
          return true;
        } catch (e) {
          toasts.error(toCommandError(e));
          return false;
        } finally {
          patchState(store, { deleting: store.deleting().filter((a) => a !== alias) });
        }
      },
    };
  }),
);

export type PresetsStore = InstanceType<typeof PresetsStore>;
