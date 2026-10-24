import type { MenuItemDef } from '@decryp7/paddock-ui/shell';
import { MAX_SUBMENU_DEPTH, type SubmenuDef } from './submenus';

/**
 * The menus as plain data, built by a pure function from the contributions (menu items and submenus) and what decides
 * their state. One model feeds the in-app menu bar, context menus and the macOS native menu bar, so they never disagree.
 * Item ids are command ids.
 */

/** A clickable item. `checked` set means a toggle (menuitemcheckbox). */
export interface MenuItemNode {
  readonly kind: 'item';
  readonly id: string;
  readonly label: string;
  /** The effective key binding (`Mod+Shift+O`), shown as the shortcut hint. */
  readonly keybinding?: string;
  readonly checked?: boolean;
  readonly enabled: boolean;
}

export interface SubmenuNode {
  readonly kind: 'submenu';
  readonly id: string;
  readonly label: string;
  readonly items: readonly MenuNode[];
}

export interface SeparatorNode {
  readonly kind: 'separator';
}

export type MenuNode = MenuItemNode | SubmenuNode | SeparatorNode;

export interface TopMenu {
  readonly id: string;
  readonly label: string;
  readonly items: readonly MenuNode[];
}

/** A command as a menu shows it now; `null` from `command()` when it is unknown or not available. */
export interface MenuCommandState {
  readonly title: string;
  readonly enabled: boolean;
  /** Set for a toggle. */
  readonly checked?: boolean;
}

/** Everything the menus are built from. */
export interface MenuSource {
  readonly items: readonly MenuItemDef[];
  readonly submenus: readonly SubmenuDef[];
  command(id: string): MenuCommandState | null;
  /** Whether a placement's own `when` holds (a throwing one hides that entry). */
  shown(entry: MenuItemDef | SubmenuDef): boolean;
  keyFor(command: string): string | undefined;
}

type Placed = { readonly group: string; readonly order: number; readonly sort: string } & ({ readonly item: MenuItemDef } | { readonly submenu: SubmenuDef });

const byString = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);

/**
 * The entries of menu `menu` (a shell menu, a submenu or a context menu id): grouped, separators between groups (a
 * group's place is its lowest `order`), entries by `order` then id; unavailable commands, hidden entries and empty
 * submenus left out; a command listed once.
 */
export function buildMenu(menu: string, src: MenuSource, depth = 0, seen: Set<string> = new Set()): MenuNode[] {
  const placed: Placed[] = [
    ...src.items.filter((i) => i.menu === menu).map((item) => ({ group: item.group, order: item.order, sort: item.command, item })),
    ...(depth < MAX_SUBMENU_DEPTH ? src.submenus.filter((s) => s.menu === menu && !seen.has(s.id)).map((submenu) => ({ group: submenu.group, order: submenu.order, sort: submenu.id, submenu })) : []),
  ];
  const groups = new Map<string, Placed[]>();
  for (const p of placed) groups.set(p.group, [...(groups.get(p.group) ?? []), p]);
  const ordered = [...groups.entries()]
    .map(([name, list]) => ({ name, list: list.sort((a, b) => a.order - b.order || byString(a.sort, b.sort)), first: Math.min(...list.map((p) => p.order)) }))
    .sort((a, b) => a.first - b.first || byString(a.name, b.name));
  const out: MenuNode[] = [];
  const listed = new Set<string>();
  for (const g of ordered) {
    const nodes: MenuNode[] = [];
    for (const p of g.list) {
      if ('item' in p) {
        if (listed.has(p.item.command) || !src.shown(p.item)) continue;
        const c = src.command(p.item.command);
        if (!c) continue;
        listed.add(p.item.command);
        const key = src.keyFor(p.item.command);
        nodes.push({ kind: 'item', id: p.item.command, label: c.title, enabled: c.enabled, ...(key ? { keybinding: key } : {}), ...(c.checked !== undefined ? { checked: c.checked } : {}) });
      } else {
        if (!src.shown(p.submenu)) continue;
        const items = buildMenu(p.submenu.id, src, depth + 1, new Set([...seen, p.submenu.id]));
        if (items.length) nodes.push({ kind: 'submenu', id: p.submenu.id, label: p.submenu.title.text, items });
      }
    }
    if (!nodes.length) continue;
    if (out.length) out.push({ kind: 'separator' });
    out.push(...nodes);
  }
  return out;
}

/** The top-level menus in the given order (`labels` names them), those left empty omitted. */
export function buildMenus(order: readonly { readonly id: string; readonly label: string }[], src: MenuSource): TopMenu[] {
  return order.map((m) => ({ id: m.id, label: m.label, items: buildMenu(m.id, src) })).filter((m) => m.items.length > 0);
}

/**
 * The in-app menu bar's menus (Windows, Linux, the browser): no app menu (the title bar already names the app, so a menu
 * titled with the app's name said it twice); its entries (Settings, About, Quit) go to the end of File, after a
 * separator, as Windows and Linux apps place them. The macOS menu bar keeps the app menu.
 */
export function foldAppMenu(menus: readonly TopMenu[], fileLabel: string): TopMenu[] {
  const app = menus.find((m) => m.id === 'app');
  if (!app) return [...menus];
  const rest = menus.filter((m) => m !== app);
  const file = rest.find((m) => m.id === 'file');
  if (!file) return [{ id: 'file', label: fileLabel, items: app.items }, ...rest];
  return rest.map((m) => (m === file ? { ...m, items: [...m.items, { kind: 'separator' } as const, ...app.items] } : m));
}

/** The items at `path` (a top menu id, then submenu ids) in `menus`; empty once it is gone. */
export function itemsAt(menus: readonly TopMenu[], path: readonly string[]): readonly MenuNode[] {
  let items: readonly MenuNode[] = menus.find((m) => m.id === path[0])?.items ?? [];
  for (const id of path.slice(1)) {
    const sub = items.find((n): n is SubmenuNode => n.kind === 'submenu' && n.id === id);
    items = sub?.items ?? [];
  }
  return items;
}
