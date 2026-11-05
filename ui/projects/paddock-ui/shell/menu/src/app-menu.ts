import { computed, EnvironmentProviders, inject, Injectable, InjectionToken, makeEnvironmentProviders, Signal } from '@angular/core';
import {
  CommandService,
  ContributionRegistry,
  evaluate,
  msg,
  provideContributionKinds,
  SHELL_MENUS,
  ShellContextHost,
  type Live,
  type Message,
  type ShellMenu,
} from '@decryp7/paddock-ui/shell';
import { QUIT_COMMAND } from '../generated/menu';
import { currentOs, currentPlatform, type MenuOs } from './keys';
import { Keymap } from './keymap';
import { buildMenu, buildMenus, foldAppMenu, type MenuNode, type MenuSource, type TopMenu } from './menu-model';
import { SUBMENUS, type SubmenuDef } from './submenus';

/** Where the menus are shown: the shell's in-app menu bar, or the macOS native menu bar (the in-app bar is then hidden). */
export type MenuMode = 'in-app' | 'native';

/** Whether this page runs inside a Tauri webview. */
export function isTauri(g: object = globalThis): boolean {
  return '__TAURI_INTERNALS__' in g;
}

/** The menu mode of this window: native in the macOS desktop app, else in-app (Windows, Linux, the browser). */
export const MENU_MODE = new InjectionToken<MenuMode>('PADDOCK_MENU_MODE', {
  providedIn: 'root',
  factory: () => (isTauri() && currentPlatform() === 'mac' ? 'native' : 'in-app'),
});

/** The titles of the shell's menus; `app` is the app's name. */
export const SHELL_MENU_TITLES: Readonly<Record<Exclude<ShellMenu, 'app'>, Message>> = {
  file: msg('pd.menu.file', 'File'),
  edit: msg('pd.menu.edit', 'Edit'),
  view: msg('pd.menu.view', 'View'),
  window: msg('pd.menu.window', 'Window'),
  help: msg('pd.menu.help', 'Help'),
};

/** What the menus need from the app. */
export interface MenuConfig {
  /** The app's name: the title of the app menu (About, Settings, Quit). */
  readonly appName: string;
}

/** The operating system, as far as menu wording goes: Windows calls the last item of the app menu "Exit". */
export const MENU_OS = new InjectionToken<MenuOs>('PADDOCK_MENU_OS', {
  providedIn: 'root',
  factory: () => currentOs(),
});

/**
 * The words of the Quit item, when the app says more than "Quit": `Quit Dyno (keeps measuring)`, so the menu tells what Quit does
 * (null: the shell's own "Quit", "Exit" on Windows). Read when the menu is built, so a signal that changes updates every menu.
 */
export const QUIT_TITLE = new InjectionToken<Signal<string | null>>('PADDOCK_QUIT_TITLE', { providedIn: 'root', factory: () => computed(() => null) });

export const MENU_CONFIG = new InjectionToken<MenuConfig>('PADDOCK_MENU_CONFIG', { providedIn: 'root', factory: () => ({ appName: 'App' }) });

/** A condition that defaults to true: absent, or not askable yet (context not loaded), it holds; throwing, it does not. */
function holds(fn: Live<object, boolean> | undefined, ctx: ReturnType<ShellContextHost['context']>): boolean {
  return !fn || evaluate(fn, ctx, ctx === null);
}

/**
 * The window's menus as one model (the menu bar, context menus and the native menu bar) and what choosing an item does.
 * Built from the contributions: menu items, submenus, the commands' availability, enabled and checked states, and the
 * effective keys.
 */
@Injectable({ providedIn: 'root' })
export class AppMenu {
  private readonly registry = inject(ContributionRegistry);
  private readonly commands = inject(CommandService);
  private readonly host = inject(ShellContextHost);
  private readonly keymap = inject(Keymap);
  private readonly config = inject(MENU_CONFIG);
  readonly mode = inject(MENU_MODE);
  private readonly os = inject(MENU_OS);
  private readonly quitTitle = inject(QUIT_TITLE);

  /** Everything the model is built from, as one value that changes when any of it does. */
  private readonly source = computed<MenuSource>(() => {
    const ctx = this.host.context();
    const available = new Map(this.commands.available().map((c) => [c.id, c]));
    // Read every key now, so the source changes with the bindings.
    const keys = new Map<string, string>();
    for (const b of this.keymap.bindings()) if (b.scope === 'window' && !keys.has(b.command)) keys.set(b.command, b.key);
    const quitTitle = this.quitTitle();
    return {
      items: this.registry.menus(),
      submenus: this.registry.items<SubmenuDef>('submenus'),
      command: (id) => {
        const c = available.get(id);
        if (!c) return null;
        return { title: c.id === QUIT_COMMAND && quitTitle ? quitTitle : c.id === QUIT_COMMAND && this.mode === 'in-app' && this.os === 'windows' ? msg('pd.app.exit', 'Exit').text : c.title.text, enabled: this.commands.isEnabled(c), ...(c.checked ? { checked: this.commands.isChecked(c) } : {}) };
      },
      shown: (entry) => holds(entry.when as Live<object, boolean> | undefined, ctx),
      keyFor: (command) => keys.get(command),
    };
  });

  /** The top-level menus: the app menu (titled with the app's name), File, Edit, View, Window, Help; empty ones left out. */
  readonly menus: Signal<readonly TopMenu[]> = computed(() => {
    const menus = buildMenus(
      SHELL_MENUS.map((id) => ({ id, label: id === 'app' ? this.config.appName : SHELL_MENU_TITLES[id].text })),
      this.source(),
    );
    // In the window (not the macOS menu bar) the app menu's entries go to File: the title bar shows the app's name.
    return this.mode === 'in-app' ? foldAppMenu(menus, SHELL_MENU_TITLES.file.text) : menus;
  });

  /** The entries of a context menu (or any menu id), current; reactive when read in a template or `computed`. */
  items(menu: string): readonly MenuNode[] {
    return buildMenu(menu, this.source());
  }

  /** Runs a menu item (from the in-app bar, a context menu or the native menu). Unknown and disabled ids do nothing. */
  run(id: string): Promise<boolean> {
    return this.commands.run(id);
  }
}

/** The menus of a window: its app name, and the submenu kind (so the menus' lint runs). Add `provideKeyboardShortcuts()` and, in the main window, `provideNativeMenu()`. */
export function provideMenus(config: MenuConfig): EnvironmentProviders {
  return makeEnvironmentProviders([{ provide: MENU_CONFIG, useValue: Object.freeze({ ...config }) }, provideContributionKinds(SUBMENUS)]);
}
