import { defineSettingsPage, msg, type ContributionBatch, type ProvidersLoader, type SettingsPageDef } from '@decryp7/paddock-ui/shell';
import { LOGIN_SECTION } from '../generated/service';
import type { ServiceCommand } from '../generated/service-commands';

// The service part's contributions: static data and loaders only (lazy by construction, plan §9). The page's component and
// the providers it needs are chunks of their own, loaded when the Settings window opens the page.

/** The id of the Background service settings page; the Rust section of the same id is `paddock_service::login::LOGIN` (`pd.login`). */
export const SERVICE_SETTINGS_PAGE = LOGIN_SECTION;

/**
 * What the Settings window may call for this page, on top of the settings commands: the Settings dialog's contract
 * lists these too (Rust's guard allows exactly the contract; write them out in the dialog, the contract check reads literals). `log_from_webview` is not needed: the page logs nothing.
 */
export const SERVICE_SETTINGS_COMMANDS = ['service_status', 'service_repair', 'service_install', 'open_login_items'] as const satisfies readonly ServiceCommand[];

export interface ServiceSettingsPageOptions {
  /**
   * The providers the page boots with: the app's `provideShellService` with its backend (`TauriServiceBackend` over its IPC),
   * from a module of its own so they stay lazy.
   */
  readonly providers: ProvidersLoader;
  /**
   * The page's component, from the registry (the entry point cannot import the page's: it is a chunk of its own, like the
   * Settings window): `() => import('@decryp7/paddock-ui/shell/service/page').then((m) => m.PdServicePage)`.
   */
  readonly load: NonNullable<SettingsPageDef['load']>;
}

/** The Background service page of the Settings window: status, Install, Repair, Open Login Items and "Start at login". */
export function defineServiceSettingsPage(options: ServiceSettingsPageOptions): SettingsPageDef {
  return defineSettingsPage({
    id: SERVICE_SETTINGS_PAGE,
    group: 'application',
    title: msg('pd.settings.service', 'Background service'),
    order: 80,
    providers: options.providers,
    load: options.load,
  });
}

/** The page as a contribution batch for the app's registry; the app also lists it among the Settings window's pages. */
export function serviceContributions(page: SettingsPageDef): ContributionBatch {
  return { settingsPages: [page] };
}
