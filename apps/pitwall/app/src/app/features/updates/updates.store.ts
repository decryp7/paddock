import { computed, effect, inject, untracked } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import type { CommandError, UpdateCheckResult, UpdateCheckStatus, UpdateNotice, UpdateSchedule } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { LiveStore } from '../../core/live.store';
import { Toasts } from '../../core/toasts';
import { bytes } from '../../shared/format';
import { Confirm } from '../../shared/ui/confirm';

interface UpdatesState {
  notices: UpdateNotice[];
  status: UpdateCheckStatus | null;
  loading: boolean;
  loaded: boolean;
  error: CommandError | null;
  /** "Check now" is running. */
  checking: boolean;
  /** The settings are being saved. */
  saving: boolean;
  /** What the last "Check now" said (kept on screen under the button). */
  lastResult: UpdateCheckResult | null;
  /** Notice ids being dismissed or downloaded. */
  busy: Record<string, true>;
}

const initial: UpdatesState = { notices: [], status: null, loading: false, loaded: false, error: null, checking: false, saving: false, lastResult: null, busy: {} };

/** The sentence a finished check is announced with. */
export function resultToast(r: UpdateCheckResult): { tone: 'ok' | 'info'; text: string } {
  if (r.outcome === 'checked') {
    return { tone: 'ok', text: r.notices === 0 ? 'Checked: your models are up to date.' : `Checked: ${r.notices} ${r.notices === 1 ? 'update' : 'updates'} available.` };
  }
  return { tone: 'info', text: r.message };
}

/**
 * Model update notices (ADR 0017): what Hugging Face offers for the models you downloaded, the check's schedule and
 * state, and the actions on a notice. The engine only tells; "Get it" asks the user to confirm, with the host, and
 * then starts the normal download.
 */
export const UpdatesStore = signalStore(
  { providedIn: 'root' },
  withState(initial),
  withComputed(({ notices, busy }) => ({
    count: computed(() => notices().length),
    /** The notices about one model (the library row's note). */
    noticesFor: computed(() => (modelId: string) => notices().filter((n) => n.modelId === modelId)),
    isBusy: computed(() => (key: string) => key in busy()),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts), confirm = inject(Confirm)) => {
    const setBusy = (key: string, on: boolean) => {
      const next = { ...store.busy() };
      if (on) next[key] = true;
      else delete next[key];
      patchState(store, { busy: next });
    };

    async function load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        const [notices, status] = await Promise.all([backend.updateNotices(), backend.updateCheckStatus()]);
        patchState(store, { notices, status, loading: false, loaded: true });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    }

    /** Re-reads quietly (a pushed event, a finished download): a failure keeps what is shown. */
    async function refresh(): Promise<void> {
      try {
        const [notices, status] = await Promise.all([backend.updateNotices(), backend.updateCheckStatus()]);
        patchState(store, { notices, status });
      } catch {
        /* the last good data stays */
      }
    }

    return {
      load,
      refresh,

      /** Loads once; the screens that show a badge call this on start. */
      async ensureLoaded(): Promise<void> {
        if (!store.loaded() && !store.loading()) await load();
      },

      /** "Check now": asks Hugging Face about the repositories you downloaded from (never in offline mode). */
      async checkNow(): Promise<void> {
        if (store.checking()) return;
        patchState(store, { checking: true });
        try {
          const r = await backend.checkUpdatesNow();
          patchState(store, { lastResult: r });
          const t = resultToast(r);
          if (t.tone === 'ok') toasts.ok(t.text);
          else toasts.show(t.text, 'info');
          await refresh();
        } catch (e) {
          toasts.error(toCommandError(e));
        } finally {
          patchState(store, { checking: false });
        }
      },

      /** Turns the scheduled check on or off and sets how often it runs. */
      async setCheck(enabled: boolean, schedule: UpdateSchedule): Promise<boolean> {
        patchState(store, { saving: true });
        try {
          const status = await backend.setUpdateCheck(enabled, schedule);
          patchState(store, { status, saving: false });
          toasts.ok(enabled ? `Checking ${schedule === 'daily' ? 'every day' : 'every week'}.` : 'Scheduled checks are off.');
          return true;
        } catch (e) {
          patchState(store, { saving: false });
          toasts.error(toCommandError(e));
          return false;
        }
      },

      /** Hides a notice until the offer changes (`snoozeDays` null) or for that many days. */
      async dismiss(id: string, snoozeDays: number | null): Promise<void> {
        if (store.isBusy()(id)) return;
        setBusy(id, true);
        try {
          const notices = await backend.dismissUpdate(id, snoozeDays);
          patchState(store, { notices });
          if (store.status()) patchState(store, { status: { ...store.status()!, notices: notices.length } });
        } catch (e) {
          toasts.error(toCommandError(e));
        } finally {
          setBusy(id, false);
        }
      },

      /**
       * "Get it": shows what would be downloaded and from which host and, only when the user confirms, starts the
       * normal download. Never automatic.
       */
      async get(n: UpdateNotice): Promise<boolean> {
        if (store.isBusy()(n.id)) return false;
        const host = store.status()?.host ?? 'huggingface.co';
        const replaces = n.kind === 'newerRevision' ? ' It replaces the copy you have once it finishes and is verified.' : '';
        const ok = await confirm.ask({
          title: `Download ${n.file}?`,
          message: `${bytes(n.sizeBytes)} from ${host} (repository ${n.repo}).${replaces} Pitwall downloads nothing until you confirm.`,
          confirm: 'Download',
        });
        if (!ok) return false;
        setBusy(n.id, true);
        try {
          await backend.downloadModel(n.repo, n.file, null);
          toasts.ok(`Downloading ${n.file}.`);
          patchState(store, { notices: store.notices().filter((x) => x.id !== n.id) });
          return true;
        } catch (e) {
          toasts.error(toCommandError(e));
          return false;
        } finally {
          setBusy(n.id, false);
        }
      },
    };
  }),
  withHooks((store, live = inject(LiveStore)) => ({
    onInit() {
      // A pushed "model update" event, a finished download or a change of offline mode changes what is shown.
      let seen = '';
      effect(() => {
        const update = live.events().find((e) => e.kind === 'modelUpdate');
        const done = live.downloads().filter((d) => d.state === 'done').length;
        // Offline mode flips what a check can do, so the state is read again when it changes.
        const key = `${update?.id ?? ''}|${done}|${live.server()?.offline ?? ''}`;
        untracked(() => {
          const changed = key !== seen;
          seen = key;
          if (changed && store.loaded()) void store.refresh();
        });
      });
    },
  })),
);

export type UpdatesStore = InstanceType<typeof UpdatesStore>;
