import { EnvironmentProviders, inject, Injectable, InjectionToken, Injector, makeEnvironmentProviders, signal, type Provider, type Type, type WritableSignal } from '@angular/core';
import { Title } from '@angular/platform-browser';
import { provideRouter, ROUTES, TitleStrategy, withHashLocation, type RouterStateSnapshot, type Route, type Routes } from '@angular/router';
import {
  ContributionRegistry,
  evaluate,
  provideContributions,
  provideShellIcon,
  REGION_KEYS,
  SHELL_PORTS,
  ShellContextHost,
  type PageDef,
  type ShellPorts,
} from '@decryp7/paddock-ui/shell';
import { openDialogPort, provideDialogs, type DialogsOptions } from '@decryp7/paddock-ui/shell/dialogs';
import { JOBS_CONTRIBUTIONS, provideShellJobs, type ShellJobsOptions } from '@decryp7/paddock-ui/shell/jobs';
import { provideKeyboardShortcuts, provideMenus, provideNativeMenu, RESERVED_KEYS, type NativeMenuPort, type ReservedKey } from '@decryp7/paddock-ui/shell/menu';
import { NOTIFY_CONTRIBUTIONS, notifyPort, provideShellNotify, type ShellNotifyOptions } from '@decryp7/paddock-ui/shell/notify';
import {
  AFTER_READY,
  provideStartupBackend,
  provideStartupContract,
  provideUiCheckpoint,
  WindowRecovery,
  type ReadyReply,
  type RetryOptions,
  type StartupBackend,
  type UiCheckpoint,
} from '@decryp7/paddock-ui/shell/startup';
import { PdPageHost } from './page-host';
import { PAGE_DATA, PAGES_CONFIG, pageOrder, pagePath, ShellPages, type PagesConfig } from './pages';
import { PAGES_SIDEBAR_SIZES } from './pages-frame.store';
import { sidebarSizes, type SidebarSizes } from './sidebar-plan';

/** Everything a manager app's main window needs besides its contributions (plan S14). */
export interface ShellPagesOptions {
  /** The app's name: the title bar, the app menu (About, Settings, Quit) and the window title ("Models – Pitwall"). */
  readonly appName: string;
  /** Service health and the notification centre (plan S4): `new TauriServiceHealthBackend(ipc)` and its sibling in the app. */
  readonly notify: Pick<ShellNotifyOptions, 'health' | 'notifications'>;
  /** The window's jobs (plan S9); without it the frame has no jobs status item. */
  readonly jobs?: ShellJobsOptions;
  /** The start-up contract (plan S7): its backend, optionally the UI state a recovery reload keeps, and the retry. */
  readonly startup: {
    readonly backend: () => StartupBackend;
    readonly checkpoint?: () => UiCheckpoint;
    readonly retry?: RetryOptions;
  };
  /** The macOS app's native menu bar (`tauriNativeMenuPort(ipc)`, plan S13); built once the window reported ready. */
  readonly nativeMenu?: NativeMenuPort;
  /** Dialogs (plan S6): `{ io: () => tauriDialogIo(bridge) }` on the desktop; in the page without it. */
  readonly dialogs?: DialogsOptions;
  /** Routes besides the pages (a detail route, the tray popover's `tray`); matched before the pages. */
  readonly routes?: Routes;
  /** Route paths by page id where the default (`pw.models` is `models`) does not suit. */
  readonly paths?: Readonly<Record<string, string>>;
  /** Hash URLs (`#/models`, the default: a Tauri window serves files) or path URLs. */
  readonly location?: 'hash' | 'path';
  /** The sidebar plan's sizes (px). */
  readonly sidebar?: Partial<SidebarSizes>;
  /** The icon component of pages and the frame (a `name` input; typically wrapping `mat-icon`). */
  readonly icon?: Type<unknown>;
  /** Keys the app handles itself, reserved besides the frame's F6 and Shift+F6. */
  readonly reservedKeys?: readonly ReservedKey[];
}

/** Whether this window has reported ready (the native menu bar is built after it). */
export const PAGES_WINDOW_READY = new InjectionToken<WritableSignal<boolean>>('PADDOCK_PAGES_WINDOW_READY', { providedIn: 'root', factory: () => signal(false) });

/** Window titles: "Models – Pitwall"; the app's name alone on a route without a title. */
@Injectable()
export class PagesTitleStrategy extends TitleStrategy {
  private readonly title = inject(Title);
  private readonly config = inject(PAGES_CONFIG);

  override updateTitle(snapshot: RouterStateSnapshot): void {
    const page = this.buildTitle(snapshot);
    this.title.setTitle(page ? `${page} – ${this.config.appName}` : this.config.appName);
  }
}

/** The route of one page: the page host, its title, and a guard for its `when` (the app's context is loaded first). */
export function pageRoute(page: PageDef, paths: Readonly<Record<string, string>>): Route {
  const when = page.when;
  return {
    path: pagePath(page.id, paths),
    component: PdPageHost,
    title: page.title.text,
    data: { [PAGE_DATA]: page.id },
    ...(when
      ? {
          canMatch: [
            () =>
              inject(ShellContextHost)
                .load()
                .then((ctx) => evaluate(when, ctx, false))
                .catch(() => false),
          ],
        }
      : {}),
  };
}

/** The routes of the window: the app's own, every page, and the first page for the empty path and anything unknown. */
export function pagesRoutes(pages: readonly PageDef[], paths: Readonly<Record<string, string>>, extra: Routes = []): Routes {
  const toDefault = () => inject(ShellPages).defaultUrl();
  return [
    { path: '', pathMatch: 'full', redirectTo: toDefault },
    ...extra,
    ...[...pages].sort(pageOrder).map((p) => pageRoute(p, paths)),
    { path: '**', redirectTo: toDefault },
  ];
}

/**
 * The main window of a manager app on the pages frame, from its contributions (ADR 0012, plan S14): routes for the
 * contributed pages (each lazy, with its own providers), the window's ports (`showPage`, `openDialog`, `notify`), menus and
 * keys (S13; F6 reserved for the regions), notifications and service health with the host placed by the frame (S4), jobs
 * (S9), the start-up contract (S7) and dialogs (S6). Put `<pd-shell-frame />` as the root component's template and
 * register the app's contributions and context next to it:
 *
 * ```ts
 * providers: [
 *   provideContributions<PitwallContext>(PITWALL_CONTRIBUTIONS),
 *   provideShellContext<PitwallContext>(() => import('./app-context').then((m) => m.pitwallContext)),
 *   provideShellPages({ appName: 'Pitwall', notify: { health, notifications }, jobs: { backend }, startup: { backend } }),
 * ]
 * ```
 */
export function provideShellPages(options: ShellPagesOptions): EnvironmentProviders {
  const config: PagesConfig = Object.freeze({ appName: options.appName, paths: Object.freeze({ ...(options.paths ?? {}) }) });
  const providers: (Provider | EnvironmentProviders)[] = [
    { provide: PAGES_CONFIG, useValue: config },
    { provide: PAGES_SIDEBAR_SIZES, useValue: sidebarSizes(options.sidebar) },
    provideRouter([], ...(options.location === 'path' ? [] : [withHashLocation()])),
    { provide: ROUTES, multi: true, useFactory: () => pagesRoutes(inject(ContributionRegistry).pages(), config.paths, options.routes) },
    { provide: TitleStrategy, useClass: PagesTitleStrategy },
    ...(options.icon ? [provideShellIcon(options.icon)] : []),
    // The shell's own contributions: the service chip and the notifications (S4), the jobs (S9).
    provideContributions(NOTIFY_CONTRIBUTIONS),
    ...(options.jobs ? [provideContributions(JOBS_CONTRIBUTIONS), provideShellJobs(options.jobs)] : []),
    provideMenus({ appName: options.appName }),
    provideKeyboardShortcuts(),
    { provide: RESERVED_KEYS, useValue: Object.freeze([...REGION_KEYS, ...(options.reservedKeys ?? [])]) },
    provideNativeMenu({ ...(options.nativeMenu ? { port: options.nativeMenu } : {}), gate: () => inject(PAGES_WINDOW_READY).asReadonly() }),
    provideShellNotify({ ...options.notify, ports: false, mountHost: false }),
    provideStartupBackend(options.startup.backend),
    provideStartupContract(options.startup.retry ? { retry: options.startup.retry } : {}),
    ...(options.startup.checkpoint ? [provideUiCheckpoint(options.startup.checkpoint)] : []),
    {
      provide: AFTER_READY,
      useFactory: () => {
        const recovery = inject(WindowRecovery);
        const ready = inject(PAGES_WINDOW_READY);
        return (reply: ReadyReply | null) => {
          void recovery.afterReady(reply);
          ready.set(true);
        };
      },
    },
    provideDialogs(options.dialogs ?? {}),
    {
      provide: SHELL_PORTS,
      useFactory: (): ShellPorts => {
        // ShellPages needs the context host, which needs these ports: it is looked up on first use.
        const injector = inject(Injector);
        return { showPage: (id) => void injector.get(ShellPages).show(id), openDialog: openDialogPort(), notify: notifyPort() };
      },
    },
  ];
  return makeEnvironmentProviders(providers);
}
