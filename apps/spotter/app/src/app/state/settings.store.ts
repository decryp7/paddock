import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { AppInfo, AppUpdateCheck, AppUpdateStatus, CommandError, MailStatus, OpenTarget, RendererStatus, Settings, SmtpSettings } from '../core/api.types';
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
  /** Whether e-mail can be sent (the password itself is never held here). */
  mail: MailStatus | null;
  mailBusy: boolean;
  /** What went wrong with the last e-mail save or test, shown next to the form; null when nothing did. */
  mailError: CommandError | null;
  /** What the last e-mail save or test achieved, for a calm confirmation. */
  mailDone: string | null;
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
    mail: null,
    mailBusy: false,
    mailError: null,
    mailDone: null,
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
        await this.loadMail();
      },
      /** Saves a change; the engine validates and its reply is what the page shows. Resolves to whether it was saved. */
      async change(patch: Partial<Settings>): Promise<boolean> {
        const current = store.settings().value;
        if (!current) return false;
        // The choice shows at once; a refusal puts the saved values back.
        const mine = ++latestChange;
        patchState(store, { settings: { value: { ...current, ...patch }, loading: false, error: null } });
        if (patch.theme) theme.setPreference(patch.theme);
        try {
          const saved = await backend.saveSettings({ ...current, ...patch });
          // A newer change is already shown (and saved after this one): its reply is the one that counts.
          if (mine === latestChange) applySaved(saved);
          return true;
        } catch (e) {
          const err = fail('Settings were not saved', e);
          if (mine !== latestChange) return false;
          patchState(store, { settings: { value: current, loading: false, error: null }, problems: err.problems });
          theme.setPreference(current.theme);
          return false;
        }
      },
      async loadMail(): Promise<void> {
        try {
          patchState(store, { mail: await backend.mailStatus() });
        } catch {
          patchState(store, { mail: null });
        }
      },
      /**
       * Saves the mail server and, when given, its password (null removes it; undefined leaves it). A refusal is kept in
       * `mailError` to show beside the form instead of a toast, because the person is looking at the fields to fix.
       */
      async saveMail(smtp: SmtpSettings | null, password?: string | null): Promise<boolean> {
        patchState(store, { mailBusy: true, mailError: null, mailDone: null });
        try {
          const current = store.settings().value;
          if (!current) return false;
          const saved = await backend.saveSettings({ ...current, smtp });
          applySaved(saved);
          const mail = password === undefined ? await backend.mailStatus() : await backend.setMailPassword(password);
          patchState(store, { mail, mailDone: smtp ? 'Mail settings saved.' : 'Mail settings cleared.' });
          return true;
        } catch (e) {
          patchState(store, { mailError: toCommandError(e) });
          return false;
        } finally {
          patchState(store, { mailBusy: false });
        }
      },
      /** Sends one message through the saved mail settings; the typed error says what to check. */
      async sendTestEmail(to: string): Promise<void> {
        patchState(store, { mailBusy: true, mailError: null, mailDone: null });
        try {
          await backend.sendTestEmail(to);
          patchState(store, { mailDone: `Test e-mail sent to ${to}.` });
        } catch (e) {
          patchState(store, { mailError: toCommandError(e) });
        } finally {
          patchState(store, { mailBusy: false });
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
