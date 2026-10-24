import { EnvironmentProviders, inject, InjectionToken, Injector, makeEnvironmentProviders, type Provider, type Type } from '@angular/core';
import { defineCommand, defineKeybinding, defineMenuItem, msg, provideContributions, provideShellIcon, REGION_KEYS, SHELL_PORTS, type ShellPorts } from '@decryp7/paddock-ui/shell';
import { openDialogPort, provideDialogs, type DialogsOptions } from '@decryp7/paddock-ui/shell/dialogs';
import { provideNativeFeel } from '@decryp7/paddock-ui/shell/feel';
import { JOBS_CONTRIBUTIONS, provideShellJobs, type ShellJobsOptions } from '@decryp7/paddock-ui/shell/jobs';
import { provideKeyboardShortcuts, provideMenus, provideNativeMenu, RESERVED_KEYS, type NativeMenuPort, type ReservedKey } from '@decryp7/paddock-ui/shell/menu';
import { NOTIFY_CONTRIBUTIONS, notifyPort, provideShellNotify, type ShellNotifyOptions } from '@decryp7/paddock-ui/shell/notify';
import { provideStartupBackend, provideStartupContract, provideUiCheckpoint, type RetryOptions, type StartupBackend, type UiCheckpoint } from '@decryp7/paddock-ui/shell/startup';
import { defaultWorkbenchPorts, defineView, DOCK_PORT, provideLiveContributions, provideWorkbenchContributions, WORKBENCH_PORTS, WorkbenchRegistry, type WorkbenchPorts } from '@decryp7/paddock-ui/workbench';
import { ExplorerStore } from '@decryp7/paddock-ui/workbench/explorer';
import { PaletteStore, providePalette, type PaletteOptions } from '@decryp7/paddock-ui/workbench/palette';
import { ActivityItems } from './activity';
import { WorkbenchFrameStore } from './frame.store';

/** What the frame needs to know about the app. */
export interface WorkbenchConfig {
  readonly appName: string;
}

export const WORKBENCH_CONFIG = new InjectionToken<WorkbenchConfig>('PADDOCK_WORKBENCH_CONFIG', { providedIn: 'root', factory: () => ({ appName: '' }) });

/** Everything an IDE-style app's main window needs besides its contributions and its dock (plan W4). */
export interface ShellWorkbenchOptions {
  /** The app's name: the title bar, the app menu, the window title. */
  readonly appName: string;
  /** Service health and the notification centre (plan S4). */
  readonly notify: Pick<ShellNotifyOptions, 'health' | 'notifications'>;
  /** The window's jobs (plan S9): a Jobs status item and the Jobs view (`pd.jobs`, a status-area pane). */
  readonly jobs?: ShellJobsOptions;
  /** The start-up contract (plan S7). */
  readonly startup: {
    readonly backend: () => StartupBackend;
    readonly checkpoint?: () => UiCheckpoint;
    readonly retry?: RetryOptions;
  };
  /** The macOS app's native menu bar (plan S13). */
  readonly nativeMenu?: NativeMenuPort;
  /** Dialogs (plan S6). */
  readonly dialogs?: DialogsOptions;
  /** The icon component (a `name` input). */
  readonly icon?: Type<unknown>;
  /** Keys the app handles itself, reserved besides F6 and Shift+F6. */
  readonly reservedKeys?: readonly ReservedKey[];
  /** The UI native-feel guard; on by default. */
  readonly nativeFeel?: boolean;
  /** The palette's options (common commands). */
  readonly palette?: PaletteOptions;
}

/** Command ids of the frame. */
export const FRAME_COMMANDS = { toggleSidebar: 'pd.wb.sidebar.toggle', jobs: 'pd.wb.jobs.show' } as const;

/** The Jobs view: the shell's jobs list as a status-area pane (ADR 0013 §1; plan §5.8). */
export const JOBS_VIEW = defineView({
  id: 'pd.jobs',
  title: msg('pd.wb.view.jobs', 'Jobs'),
  icon: 'pd:activity',
  layout: { area: 'status', min: { width: 320, height: 180 }, pref: { width: 480, height: 320 }, priority: 20 },
  order: 900,
  load: () => import('@decryp7/paddock-ui/shell/jobs').then((m) => m.PdJobsList),
});

/**
 * The window's ports (ADR 0012 §2; plan W4): the dock's part from `DOCK_PORT` (provided by `provideWorkbenchDock`), the
 * Explorer's and the palette's from the frame. `showPage` of the shell's ports shows a view in a workbench window.
 */
export function workbenchPorts(injector: Injector): WorkbenchPorts {
  const fallback = defaultWorkbenchPorts();
  const dock = () => injector.get(DOCK_PORT) ?? fallback;
  const frame = () => injector.get(WorkbenchFrameStore);
  return {
    showView: (id, options) => dock().showView(id, options),
    closeView: (panel) => dock().closeView(panel),
    get openPanels() {
      return dock().openPanels;
    },
    get activePanel() {
      return dock().activePanel;
    },
    applyPerspective: (id) => dock().applyPerspective(id),
    get activePerspective() {
      return dock().activePerspective;
    },
    showExplorer: (id) => frame().show(id),
    hideExplorer: () => frame().hide(),
    get explorerShown() {
      return frame().explorerShown;
    },
    openPalette: (prefix) => injector.get(PaletteStore).open(prefix),
  } satisfies WorkbenchPorts;
}

/**
 * The main window of an IDE-style app (ADR 0013; plan W4): the frame's stores (side bar, Explorer, activity bar), the
 * palette (W5), the window's ports, menus and keys, notifications and health, jobs and the Jobs view, the start-up
 * contract and dialogs. Put `<pd-workbench><pd-dock slot="stage" /></pd-workbench>` as the root component's template,
 * with the dock's and the app's providers beside this:
 *
 * ```ts
 * providers: [
 *   provideWorkbenchContributions<PacenotesContext>(PACENOTES_CONTRIBUTIONS),
 *   provideShellContext<PacenotesContext>(() => import('./app-context').then((m) => m.pacenotesContext)),
 *   provideShellWorkbench({ appName: 'Pacenotes', notify: { health, notifications }, jobs: { backend }, startup: { backend } }),
 *   provideWorkbenchDock({ store: ipcLayoutStore(invoke) }),
 * ]
 * ```
 */
export function provideShellWorkbench(options: ShellWorkbenchOptions): EnvironmentProviders {
  const providers: (Provider | EnvironmentProviders)[] = [
    { provide: WORKBENCH_CONFIG, useValue: Object.freeze({ appName: options.appName }) },
    WorkbenchFrameStore,
    ExplorerStore,
    ActivityItems,
    { provide: WORKBENCH_PORTS, useFactory: () => workbenchPorts(inject(Injector)) },
    providePalette(options.palette ?? {}),
    ...(options.icon ? [provideShellIcon(options.icon)] : []),
    provideContributions(NOTIFY_CONTRIBUTIONS),
    ...(options.jobs ? [provideContributions(JOBS_CONTRIBUTIONS), provideShellJobs(options.jobs), provideWorkbenchContributions({ views: [JOBS_VIEW] })] : []),
    provideLiveContributions((injector) => ({
      commands: [
        defineCommand({
          id: FRAME_COMMANDS.toggleSidebar,
          title: msg('pd.wb.cmd.toggle-sidebar', 'Toggle side bar'),
          category: msg('pd.wb.category.view', 'View'),
          checked: () => injector.get(WorkbenchFrameStore).explorerShown() !== null,
          run: () => {
            const frame = injector.get(WorkbenchFrameStore);
            if (frame.explorerShown()) frame.hide();
            else {
              const registry = injector.get(WorkbenchRegistry);
              const first = registry.availableExplorerProviders()[0]?.id;
              if (first) frame.show(frame.wanted() ?? first);
            }
          },
        }),
        ...(options.jobs
          ? [
              defineCommand({
                id: FRAME_COMMANDS.jobs,
                title: msg('pd.wb.cmd.jobs', 'Show jobs'),
                category: msg('pd.wb.category.view', 'View'),
                run: () => void injector.get(WORKBENCH_PORTS).showView(JOBS_VIEW.id),
              }),
            ]
          : []),
      ],
      keybindings: [defineKeybinding({ command: FRAME_COMMANDS.toggleSidebar, key: 'Mod+B' })],
      menus: [defineMenuItem({ menu: 'view', command: FRAME_COMMANDS.toggleSidebar, group: 'layout', order: 200 })],
    })),
    provideMenus({ appName: options.appName }),
    provideKeyboardShortcuts(),
    { provide: RESERVED_KEYS, useValue: Object.freeze([...REGION_KEYS, ...(options.reservedKeys ?? [])]) },
    provideNativeMenu(options.nativeMenu ? { port: options.nativeMenu } : {}),
    provideShellNotify({ ...options.notify, ports: false, mountHost: false }),
    provideStartupBackend(options.startup.backend),
    provideStartupContract(options.startup.retry ? { retry: options.startup.retry } : {}),
    ...(options.startup.checkpoint ? [provideUiCheckpoint(options.startup.checkpoint)] : []),
    provideDialogs(options.dialogs ?? {}),
    ...(options.nativeFeel === false ? [] : [provideNativeFeel()]),
    {
      provide: SHELL_PORTS,
      useFactory: (): ShellPorts => {
        const injector = inject(Injector);
        // A "page" of a workbench window is a view; looked up on first use (the ports need the context host).
        return { showPage: (id) => void injector.get(WORKBENCH_PORTS).showView(id), openDialog: openDialogPort(), notify: notifyPort() };
      },
    },
  ];
  return makeEnvironmentProviders(providers);
}

