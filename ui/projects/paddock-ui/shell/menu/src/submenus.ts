import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import {
  CONTRIBUTION_ID,
  contributionProviders,
  defineContributionKind,
  isMessage,
  provideContributionKinds,
  SHELL_MENUS,
  type Live,
  type MenuItemDef,
  type Message,
} from '@decryp7/paddock-ui/shell';

/** Submenus nest at most this deep below a top-level menu (`paddock-menu`'s `MAX_DEPTH` counts the top items as 1). */
export const MAX_SUBMENU_DEPTH = 3;

/**
 * A submenu: a namespaced menu id (`pw.models.recent`) with a title, placed in a shell menu or another submenu like a
 * menu item. Menu items name it as their `menu`. A namespaced menu id that is not a submenu is a context menu
 * (`pd-context-menu`), never shown in the menu bar.
 */
export interface SubmenuDef<T extends object = object> {
  readonly id: string;
  /** The menu it is in: a shell menu (`view`) or another submenu's id. */
  readonly menu: string;
  readonly title: Message;
  /** Separators fall between groups (as for menu items). */
  readonly group: string;
  readonly order: number;
  readonly when?: Live<T, boolean>;
}

export function defineSubmenu<T extends object = object>(spec: Omit<SubmenuDef<T>, 'group'> & { readonly group?: string }): SubmenuDef<T> {
  return Object.freeze({ ...spec, group: spec.group ?? 'main' });
}

const isShellMenu = (m: unknown): boolean => (SHELL_MENUS as readonly unknown[]).includes(m);

/** The submenu kind for the contributions registry; its lint also checks the menus as a whole (one place per command). */
export const SUBMENUS = defineContributionKind<SubmenuDef>({
  name: 'submenus',
  label: 'submenu',
  identity: (s) => s.id,
  check: (s) =>
    [
      (typeof s.id !== 'string' || !CONTRIBUTION_ID.test(s.id)) && `id ${JSON.stringify(s.id)} is not a namespaced id (pw.models.recent)`,
      !isMessage(s.title) && `title has no message id (use msg('<ns>.<name>', 'Text'))`,
      !isShellMenu(s.menu) && (typeof s.menu !== 'string' || !CONTRIBUTION_ID.test(s.menu)) && `menu ${JSON.stringify(s.menu)} is neither a shell menu nor a submenu id`,
      (typeof s.group !== 'string' || !/^[a-z][a-z0-9-]*$/.test(s.group)) && `group ${JSON.stringify(s.group)} is malformed`,
      (typeof s.order !== 'number' || !Number.isFinite(s.order) || s.order < 0 || s.order > 100_000) && `order must be a number from 0 to 100000`,
    ].filter((p): p is string => typeof p === 'string'),
  checkAll: (subs, lookup) => {
    const out: string[] = [];
    const byId = new Map(subs.map((s) => [s.id, s]));
    for (const s of subs) {
      if (!isShellMenu(s.menu) && !byId.has(s.menu)) out.push(`submenu ${s.id} is in the missing menu ${s.menu}`);
      // Walk up to a shell menu: a cycle or a chain deeper than the native menu bar allows is a defect.
      const chain = [s.id];
      let at: SubmenuDef | undefined = s;
      while (at && !isShellMenu(at.menu)) {
        if (chain.includes(at.menu)) {
          out.push(`submenu ${s.id} is inside itself (${[...chain, at.menu].join(' > ')})`);
          break;
        }
        chain.push(at.menu);
        at = byId.get(at.menu);
      }
      if (chain.length > MAX_SUBMENU_DEPTH) out.push(`submenu ${s.id} is nested ${chain.length} deep; at most ${MAX_SUBMENU_DEPTH} are allowed`);
    }
    // A command is placed in one menu-bar menu only: the native menu bar shows an id once (`paddock-menu` refuses a
    // duplicate). Context menus may repeat a menu-bar command.
    const inBar = (menu: string): boolean => isShellMenu(menu) || byId.has(menu);
    const placed = new Map<string, string>();
    for (const m of lookup.items('menus') as readonly MenuItemDef[]) {
      if (!m || typeof m !== 'object' || !inBar(m.menu)) continue;
      const first = placed.get(m.command);
      if (first !== undefined && first !== m.menu) out.push(`command ${m.command} is in two menus (${first} and ${m.menu}); the native menu bar shows a command once`);
      else placed.set(m.command, m.menu);
    }
    return out;
  },
});

/**
 * Registers submenus (and the submenu kind, once however often it is called): `provideSubmenus(defineSubmenu({...}))`
 * next to the feature's `provideContributions`.
 */
export function provideSubmenus<T extends object = object>(...submenus: readonly SubmenuDef<T>[]): EnvironmentProviders {
  return makeEnvironmentProviders([provideContributionKinds(SUBMENUS), ...contributionProviders('submenus', submenus)]);
}
