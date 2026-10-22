import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withProps, withState } from '@ngrx/signals';
import type { CommandError, ConfigEdit, ConfigPreview, ConfigSaved, RunnerConfigReply, RunnerSettings } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { idle, pending, settle, type Loadable } from '../../state/loadable';
import { ToastStore } from '../../state/toast.store';
import { errorCount } from './config/config-issues';
import { latestLabelCheck, type LabelCheck } from './config/label-check';

/** How long the form waits after the last keystroke before asking the engine to validate. */
export const PREVIEW_DEBOUNCE_MS = 300;

export type ConfigMode = 'form' | 'raw';

interface ConfigState {
  runner: string | null;
  reply: Loadable<RunnerConfigReply>;
  mode: ConfigMode;
  /** The form's working copy. */
  settings: RunnerSettings | null;
  rawText: string;
  /** The text the Advanced editor started with; typing anything else makes the file "edited directly". */
  rawEntry: string;
  preview: ConfigPreview | null;
  previewing: boolean;
  previewError: CommandError | null;
  saving: boolean;
  /** The file changed on disk since it was opened: reload to continue. */
  conflict: string | null;
  saveError: CommandError | null;
  saved: ConfigSaved | null;
  labelCheck: LabelCheck | null;
}

const initial: ConfigState = {
  runner: null,
  reply: idle<RunnerConfigReply>(),
  mode: 'form',
  settings: null,
  rawText: '',
  rawEntry: '',
  preview: null,
  previewing: false,
  previewError: null,
  saving: false,
  conflict: null,
  saveError: null,
  saved: null,
  labelCheck: null,
};

/** Whether a rejected save means the file changed since it was read. */
function isConflict(e: CommandError): boolean {
  return e.code === 'busy' || /changed on disk|has changed|changed since/i.test(e.message);
}

/**
 * One runner's `config.yaml` in the editor: the file as the engine read it, the form's working copy or the raw text,
 * the engine's live preview of the edit (diff and findings), and saving. Validation is never done here.
 */
export const RunnerConfigStore = signalStore(
  withState<ConfigState>(initial),
  withComputed((s) => {
    const file = computed(() => s.reply().value);
    const edit = computed<ConfigEdit | null>(() => {
      const f = file();
      const settings = s.settings();
      if (!f || !settings) return null;
      return s.mode() === 'raw' ? { mode: 'raw', text: s.rawText() } : { mode: 'form', settings };
    });
    const dirty = computed(() => {
      const f = file();
      if (!f) return false;
      return s.mode() === 'raw' ? s.rawText() !== f.text : JSON.stringify(s.settings()) !== JSON.stringify(f.view.settings);
    });
    const issues = computed(() => (dirty() ? (s.preview()?.issues ?? []) : (file()?.view.issues ?? [])));
    return {
      file,
      edit,
      dirty,
      issues,
      /** The text was edited in the Advanced editor: the form cannot show it any more. */
      rawEdited: computed(() => s.mode() === 'raw' && s.rawText() !== s.rawEntry()),
      hasErrors: computed(() => errorCount(issues()) > 0),
      /** Saving is possible: something changed, the engine saw the latest edit, and found no error. */
      canSave: computed(() => dirty() && !s.previewing() && !s.saving() && s.preview() !== null && s.preview()!.changed && errorCount(s.preview()!.issues) === 0),
    };
  }),
  // The debounce timer and the request counter are not state: nothing renders them.
  withProps(() => ({ _ctl: { timer: null as ReturnType<typeof setTimeout> | null, seq: 0 } })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(ToastStore)) => {
    const ctl = store._ctl;

    const cancelTimer = () => {
      if (ctl.timer) clearTimeout(ctl.timer);
      ctl.timer = null;
    };

    /** Asks the engine what the current edit would do (the latest answer wins). */
    const runPreview = async (): Promise<void> => {
      cancelTimer();
      const runner = store.runner();
      const edit = store.edit();
      if (!runner || !edit) return;
      const mine = ++ctl.seq;
      patchState(store, { previewing: true });
      try {
        const preview = await backend.previewRunnerConfig(runner, edit);
        if (mine === ctl.seq) patchState(store, { preview, previewing: false, previewError: null });
      } catch (e) {
        if (mine === ctl.seq) patchState(store, { previewing: false, previewError: toCommandError(e) });
      }
    };

    const schedule = () => {
      cancelTimer();
      patchState(store, { previewing: true });
      ctl.timer = setTimeout(() => void runPreview(), PREVIEW_DEBOUNCE_MS);
    };

    /** `_trigger` only exists so a caller's effect depends on what made it refresh. */
    const refreshLabelCheck = async (_trigger?: unknown): Promise<void> => {
      const runner = store.runner();
      if (!runner) return;
      try {
        const a = await backend.activity(runner, 7);
        if (store.runner() === runner) patchState(store, { labelCheck: latestLabelCheck(a.events) });
      } catch {
        // The notice is a convenience; the file itself is what matters here.
      }
    };

    const reset = (reply: RunnerConfigReply | null): Partial<ConfigState> => ({
      mode: 'form',
      settings: reply ? structuredClone(reply.view.settings) : null,
      rawText: reply?.text ?? '',
      rawEntry: reply?.text ?? '',
      preview: null,
      previewing: false,
      previewError: null,
      conflict: null,
      saveError: null,
    });

    const load = async (runner: string, keepNotice = false): Promise<void> => {
      cancelTimer();
      ctl.seq++;
      const same = store.runner() === runner;
      patchState(store, { runner, reply: pending(same ? store.reply() : undefined), ...(same ? {} : { labelCheck: null }) });
      const reply = await settle(same ? store.reply() : undefined, () => backend.runnerConfig(runner));
      if (store.runner() !== runner) return;
      patchState(store, { reply, ...(reply.value && !reply.error ? reset(reply.value) : {}), ...(keepNotice ? {} : { saved: null }) });
      void refreshLabelCheck();
    };

    return {
      load,
      refreshLabelCheck,
      /** Forgets edits and reads the file again (also the way out of a "changed on disk" conflict). */
      reload: () => {
        const r = store.runner();
        return r ? load(r) : Promise.resolve();
      },
      /** Changes some of the form's settings and asks the engine to check the result. */
      patch(change: Partial<RunnerSettings>): void {
        const s = store.settings();
        if (!s || store.mode() !== 'form') return;
        patchState(store, { settings: { ...s, ...change }, saved: null });
        schedule();
      },
      /** The Advanced editor's text. */
      setRaw(text: string): void {
        if (store.mode() !== 'raw') return;
        patchState(store, { rawText: text, saved: null });
        schedule();
      },
      /** Switches between the form and the Advanced editor. Back to the form only while the text is untouched. */
      setMode(mode: ConfigMode): void {
        if (mode === store.mode()) return;
        if (mode === 'raw') {
          const text = store.dirty() ? (store.preview()?.text ?? store.file()?.text ?? '') : (store.file()?.text ?? '');
          patchState(store, { mode: 'raw', rawText: text, rawEntry: text });
          return;
        }
        if (store.rawEdited()) return;
        patchState(store, { mode: 'form' });
        if (store.dirty()) schedule();
        else patchState(store, { preview: null, previewing: false });
      },
      /** Throws the edits away and goes back to the file as it is. */
      discard(): void {
        cancelTimer();
        ctl.seq++;
        patchState(store, reset(store.file()));
      },
      /** Gets the engine's answer for the current edit now (the review dialog needs it up to date). */
      review: runPreview,
      dismissSaved(): void {
        patchState(store, { saved: null });
      },
      /** Saves the current edit against the fingerprint of the file it was made on; true when it was saved. */
      async save(): Promise<boolean> {
        const runner = store.runner();
        const edit = store.edit();
        const file = store.file();
        if (!runner || !edit || !file || store.saving()) return false;
        cancelTimer();
        ctl.seq++;
        patchState(store, { saving: true, saveError: null, conflict: null });
        try {
          const saved = await backend.saveRunnerConfig(runner, edit, file.fingerprint);
          patchState(store, { saving: false, saved });
          toasts.show({ tone: 'success', icon: 'check-circle', title: 'Configuration saved', body: saved.changedKeys.length > 0 ? saved.changedKeys.join(', ') : null });
          await load(runner, true);
          return true;
        } catch (e) {
          const err = toCommandError(e);
          patchState(store, isConflict(err) ? { saving: false, conflict: err.message } : { saving: false, saveError: err });
          return false;
        }
      },
    };
  }),
  withHooks({
    onDestroy(store) {
      if (store._ctl.timer) clearTimeout(store._ctl.timer);
      store._ctl.seq++;
    },
  }),
);
