import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { AppInfo, AppUpdateCheck, AppUpdateStatus, CommandError, OpenTarget, RendererStatus, Settings } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { ThemeService } from '../core/theme.service';
import { idle, pending, settle, type Loadable } from './loadable';
import { ToastStore } from './toast.store';

interface SettingsState {
  settings: Loadable<Settings>;
  autostart: boolean | null;
  info: Loadable<AppInfo>;
  renderer: Loadable<RendererStatus>;
  rendererBusy: boolean;
  update: Loadable<AppUpdateStatus>;
  updateCheck: AppUpdateCheck | null;
  updateBusy: boolean;
  /** Problems the last save reported. */
  problems: string[];
}

/** Settings and everything the settings page shows about the app (info, renderer, updates). */
export const SettingsStore = signalStore(
  { providedIn: 'root' },
  withState<SettingsState>({
    settings: idle<Settings>(),
    autostart: null,
    info: idle<AppInfo>(),
    renderer: idle<RendererStatus>(),
    rendererBusy: false,
    update: idle<AppUpdateStatus>(),
    updateCheck: null,
    updateBusy: false,
    problems: [],
  }),
  withMethods((store) => {
    const backend = inject(BACKEND);
    const theme = inject(ThemeService);
    const toast = inject(ToastStore);

    const fail = (title: string, e: unknown): CommandError => {
      const err = toCommandError(e);
      toast.error(title, err.message);
      return err;
    };

    let latestChange = 0;

    const loadSettings = async (): Promise<void> => {
      const startedAt = latestChange;
      patchState(store, { settings: pending(store.settings()) });
      const next = await settle(store.settings(), () => backend.getSettings());
      // The user changed something while this read was in flight: the read is older than what is shown.
      if (startedAt !== latestChange) return;
      patchState(store, { settings: next });
      if (next.value) theme.setPreference(next.value.theme);
    };

    const applySaved = (saved: Settings) => {
      patchState(store, { settings: { value: saved, loading: false, error: null }, problems: [] });
      theme.setPreference(saved.theme);
    };

    return {
      loadSettings,
      async loadAll(): Promise<void> {
        patchState(store, { info: pending(store.info()), renderer: pending(store.renderer()), update: pending(store.update()) });
        const [info, renderer, update, autostart] = await Promise.all([
          settle(store.info(), () => backend.appInfo()),
          settle(store.renderer(), () => backend.rendererStatus()),
          settle(store.update(), () => backend.appUpdateStatus()),
          backend.getAutostart().catch(() => null),
          loadSettings(),
        ]);
        patchState(store, { info, renderer, update, autostart });
      },
      /** Saves a change; the engine validates and its reply is what the page shows. */
      async change(patch: Partial<Settings>): Promise<void> {
        const current = store.settings().value;
        if (!current) return;
        // The choice shows at once; a refusal puts the saved values back.
        const mine = ++latestChange;
        patchState(store, { settings: { value: { ...current, ...patch }, loading: false, error: null } });
        if (patch.theme) theme.setPreference(patch.theme);
        try {
          const saved = await backend.saveSettings({ ...current, ...patch });
          // A newer change is already shown (and saved after this one): its reply is the one that counts.
          if (mine === latestChange) applySaved(saved);
        } catch (e) {
          const err = fail('Settings were not saved', e);
          if (mine !== latestChange) return;
          patchState(store, { settings: { value: current, loading: false, error: null }, problems: err.problems });
          theme.setPreference(current.theme);
        }
      },
      async pauseNotifications(minutes: number | null): Promise<void> {
        try {
          applySaved(await backend.pauseNotifications(minutes));
          toast.success(minutes === null ? 'Notifications resumed' : 'Notifications paused');
        } catch (e) {
          fail('Could not change notifications', e);
        }
      },
      async setAutostart(enabled: boolean): Promise<void> {
        try {
          patchState(store, { autostart: await backend.setAutostart(enabled) });
        } catch (e) {
          fail('Could not change launch at login', e);
        }
      },
      open: (target: OpenTarget) =>
        backend.open(target).catch((e: unknown) => {
          fail('Could not open it', e);
        }),
      async chooseDownloads(): Promise<void> {
        const folder = await backend.chooseFolder().catch((e: unknown) => {
          fail('Could not open the picker', e);
          return null;
        });
        if (folder) await this.change({ downloadsFolder: folder });
      },
      async installRenderer(): Promise<void> {
        patchState(store, { rendererBusy: true });
        try {
          patchState(store, { renderer: { value: await backend.installRenderer(), loading: false, error: null } });
          toast.success('Browser installed');
        } catch (e) {
          fail('The browser could not be installed', e);
        } finally {
          patchState(store, { rendererBusy: false });
        }
      },
      async checkUpdate(): Promise<void> {
        patchState(store, { updateBusy: true });
        try {
          patchState(store, { updateCheck: await backend.checkAppUpdate() });
        } catch (e) {
          fail('Could not check for updates', e);
        } finally {
          patchState(store, { updateBusy: false });
        }
      },
      async installUpdate(): Promise<void> {
        patchState(store, { updateBusy: true });
        try {
          await backend.installAppUpdate();
        } catch (e) {
          fail('The update was not installed', e);
        } finally {
          patchState(store, { updateBusy: false });
        }
      },
    };
  }),
);
