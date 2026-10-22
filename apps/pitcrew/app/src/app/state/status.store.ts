import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import type { BackendEvent, CleanupReport, CommandError, ConnectionState, PitcrewNotification, RunnerSnapshot, StatusReply, Version } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { PreferencesService } from '../core/preferences.service';
import { formatBytes, plural } from '../shared/format';
import { notificationIcon, severityTone } from '../shared/labels';
import { ToastStore } from './toast.store';

/** What the user can ask a runner to do. */
export type RunnerCommand = 'start' | 'stop' | 'restart' | 'drain' | 'resume' | 'checkUpdate' | 'applyUpdate' | 'cancelUpdate';

const DONE: Record<RunnerCommand, string> = {
  start: 'Starting',
  stop: 'Stopping',
  restart: 'Restarting',
  drain: 'Draining',
  resume: 'Resumed',
  checkUpdate: 'Checked for updates on',
  applyUpdate: 'Updating',
  cancelUpdate: 'Cancelled the update of',
};

interface StatusState {
  status: StatusReply | null;
  connection: ConnectionState;
  loading: boolean;
  error: CommandError | null;
  notifications: PitcrewNotification[];
  /** The command in flight per runner. */
  inFlight: Record<string, RunnerCommand | 'skip' | 'pin' | 'clean' | 'remove' | undefined>;
  /** Events seen since the store started (lets pages refresh on changes without polling). */
  eventCount: number;
}

const MAX_NOTIFICATIONS = 100;

/** Every runner's status and the service's, kept current by pushed events; and the runner actions. */
export const StatusStore = signalStore(
  { providedIn: 'root' },
  withState<StatusState>({ status: null, connection: { state: 'connecting' }, loading: false, error: null, notifications: [], inFlight: {}, eventCount: 0 }),
  withComputed(({ status, connection, error }) => ({
    runners: computed<RunnerSnapshot[]>(() => status()?.runners ?? []),
    service: computed(() => status()?.service ?? null),
    /** Runners that need the user (shown as a badge). */
    attention: computed(() => (status()?.runners ?? []).filter((r) => r.status === 'crashLoop' || r.status === 'stalled' || r.status === 'notRegistered' || r.problem !== null)),
    /** Why the service cannot be reached, if it cannot. */
    offline: computed<{ kind: 'unavailable' | 'mismatch'; message: string } | null>(() => {
      const c = connection();
      if (c.state === 'unavailable' || c.state === 'mismatch') return { kind: c.state, message: c.message };
      const e = error();
      if (e?.code === 'serviceUnavailable') return { kind: 'unavailable', message: e.message };
      if (e?.code === 'protocolMismatch') return { kind: 'mismatch', message: e.message };
      return null;
    }),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(ToastStore), prefs = inject(PreferencesService)) => {
    const load = async (): Promise<void> => {
      patchState(store, { loading: true });
      try {
        const status = await backend.status();
        patchState(store, { status, loading: false, error: null });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    };

    const onEvent = (event: BackendEvent): void => {
      patchState(store, { eventCount: store.eventCount() + 1 });
      switch (event.kind) {
        case 'status':
          patchState(store, { status: event.status, error: null });
          break;
        case 'notification': {
          const n = event.notification;
          patchState(store, { notifications: [n, ...store.notifications()].slice(0, MAX_NOTIFICATIONS) });
          if (prefs.isEnabled(n.kind)) toasts.show({ tone: severityTone(n.severity), icon: notificationIcon(n.kind), title: n.title, body: n.body });
          break;
        }
        case 'connection': {
          const was = store.connection().state;
          patchState(store, { connection: event.connection });
          if (event.connection.state === 'connected' && was !== 'connected') void load();
          break;
        }
      }
    };

    const run = async (runner: string, key: StatusState['inFlight'][string], fn: () => Promise<void>, done: string | null): Promise<boolean> => {
      patchState(store, { inFlight: { ...store.inFlight(), [runner]: key } });
      try {
        await fn();
        if (done) toasts.show({ tone: 'success', icon: 'check-circle', title: done, body: null });
        return true;
      } catch (e) {
        const err = toCommandError(e);
        toasts.show({ tone: 'danger', icon: 'alert', title: `${runner}: that did not work`, body: err.message });
        return false;
      } finally {
        patchState(store, { inFlight: { ...store.inFlight(), [runner]: undefined } });
      }
    };

    return {
      load,
      /** Handles one pushed event (exposed for tests). */
      onEvent,
      /** Asks the service to do something with a runner; the result arrives as a status event. */
      act(runner: string, command: RunnerCommand): Promise<boolean> {
        return run(runner, command, () => backend[command](runner), `${DONE[command]} ${runner}`);
      },
      skipVersion(runner: string, version: Version): Promise<boolean> {
        return run(runner, 'skip', () => backend.skipVersion(runner, version), `${runner} will skip ${version}`);
      },
      pinVersion(runner: string, version: Version | null): Promise<boolean> {
        return run(runner, 'pin', () => backend.pinVersion(runner, version), version ? `${runner} is pinned to ${version}` : `${runner} follows the latest release again`);
      },
      /** Runs a manual cleanup now; resolves with its report (or null when it failed). */
      async clean(runner: string): Promise<CleanupReport | null> {
        let report: CleanupReport | null = null;
        await run(
          runner,
          'clean',
          async () => {
            report = await backend.cleanDisk(runner);
            const r: CleanupReport = report;
            toasts.show({
              tone: r.failures.length > 0 ? 'warning' : 'success',
              icon: 'broom',
              title: `Freed ${formatBytes(r.freedBytes)} on ${runner}`,
              body: r.failures.length > 0 ? `${plural(r.failures.length, 'item')} could not be deleted; see the Disk tab for why.` : `${plural(r.deleted, 'item')} deleted.`,
            });
          },
          null,
        );
        return report;
      },
      async remove(runner: string): Promise<boolean> {
        const ok = await run(runner, 'remove', () => backend.removeRunner(runner), `Removed ${runner}`);
        if (ok) await load();
        return ok;
      },
      clearNotifications(): void {
        patchState(store, { notifications: [] });
      },
      runner(name: string): RunnerSnapshot | undefined {
        return store.runners().find((r) => r.name === name);
      },
    };
  }),
  withHooks((store, backend = inject(BACKEND)) => {
    let unsubscribe: (() => void) | null = null;
    return {
      onInit() {
        unsubscribe = backend.watch((e) => store.onEvent(e));
        void store.load();
      },
      onDestroy() {
        unsubscribe?.();
      },
    };
  }),
);
