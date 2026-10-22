import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { AppInfo, AppPermissions, CommandError, CreatedKey, LocalModel } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';

interface AppsState {
  apps: AppInfo[];
  models: LocalModel[];
  loading: boolean;
  loaded: boolean;
  error: CommandError | null;
  /** Actions in flight by key (`create`, `app:<id>`, `key:<id>`). */
  busy: Record<string, true>;
}

const initial: AppsState = { apps: [], models: [], loading: false, loaded: false, error: null, busy: {} };

/** Replaces the app with the same id (or appends it). */
export function upsertApp(list: AppInfo[], app: AppInfo): AppInfo[] {
  const i = list.findIndex((a) => a.id === app.id);
  if (i < 0) return [...list, app];
  const copy = list.slice();
  copy[i] = app;
  return copy;
}

/** Apps & keys: the apps with their keys, usage and permissions, and the local models the permissions can name. */
export const AppsStore = signalStore(
  withState(initial),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => {
    const setBusy = (key: string, on: boolean) => {
      const next = { ...store.busy() };
      if (on) next[key] = true;
      else delete next[key];
      patchState(store, { busy: next });
    };

    async function act<T>(key: string, fn: () => Promise<T>, ok?: string): Promise<T | undefined> {
      if (key in store.busy()) return undefined;
      setBusy(key, true);
      try {
        const v = await fn();
        if (ok) toasts.ok(ok);
        return v;
      } catch (e) {
        toasts.error(toCommandError(e));
        return undefined;
      } finally {
        setBusy(key, false);
      }
    }

    return {
      isBusy(key: string): boolean {
        return key in store.busy();
      },

      /** Loads apps and local models; a failure keeps the last good list and shows the error with Retry. */
      async load(): Promise<void> {
        patchState(store, { loading: true, error: null });
        try {
          const [apps, models] = await Promise.all([backend.apps(), backend.localModels()]);
          patchState(store, { apps, models, loading: false, loaded: true });
        } catch (e) {
          patchState(store, { loading: false, error: toCommandError(e) });
        }
      },

      async createApp(name: string): Promise<AppInfo | undefined> {
        const app = await act('create', () => backend.createApp(name), `Created ${name.trim()}.`);
        if (app) patchState(store, { apps: upsertApp(store.apps(), app) });
        return app;
      },

      async deleteApp(id: string): Promise<boolean> {
        const done = await act(`app:${id}`, () => backend.deleteApp(id).then(() => true), 'App deleted. Its keys no longer work.');
        if (done) patchState(store, { apps: store.apps().filter((a) => a.id !== id) });
        return !!done;
      },

      /** Creates a key; the secret is only in the returned value (shown once, never kept in the store). */
      async createKey(appId: string): Promise<CreatedKey | undefined> {
        const created = await act(`app:${appId}`, () => backend.createKey(appId));
        if (created) {
          patchState(store, { apps: store.apps().map((a) => (a.id === appId ? { ...a, keys: [...a.keys, created.key] } : a)) });
        }
        return created;
      },

      async revokeKey(appId: string, keyId: string): Promise<boolean> {
        const done = await act(`key:${keyId}`, () => backend.revokeKey(appId, keyId).then(() => true), 'Key revoked.');
        if (done) patchState(store, { apps: store.apps().map((a) => (a.id === appId ? { ...a, keys: a.keys.filter((k) => k.id !== keyId) } : a)) });
        return !!done;
      },

      async setPermissions(appId: string, permissions: AppPermissions): Promise<boolean> {
        const app = await act(`app:${appId}`, () => backend.setPermissions(appId, permissions), 'Permissions saved.');
        if (app) patchState(store, { apps: upsertApp(store.apps(), app) });
        return !!app;
      },
    };
  }),
);

export type AppsStore = InstanceType<typeof AppsStore>;
