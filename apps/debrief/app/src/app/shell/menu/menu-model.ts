import { ANALYSIS_MENU_ORDER } from '../../analysis/analysis-menu-order';
import { ViewCategory } from '../../extensions/tokens';
import { ThemeId } from '../../theme/theme.types';
import { DEBRIEF_MENU_ITEMS, type DebriefMenuItemDef } from './menu.contributions';

/**
 * The application menus as plain data, built by a pure function from what is registered (views, commands) and the
 * state that decides check marks. One model feeds both the in-app menu bar (Windows, Linux, browser) and the macOS
 * native menu bar, so they never disagree.
 *
 * Item ids are command ids, or one of the prefixed ids below for items that are not commands.
 */

/** A clickable item. `checked` set means it is a toggle (menuitemcheckbox). */
export interface MenuItemNode {
  kind: 'item';
  id: string;
  label: string;
  /** Key binding in command syntax (e.g. "Mod+Shift+O"), shown as the shortcut hint. */
  keybinding?: string;
  checked?: boolean;
}

export interface SubmenuNode {
  kind: 'submenu';
  id: string;
  label: string;
  items: MenuNode[];
}

export interface SeparatorNode {
  kind: 'separator';
}

/** A labelled section of items (e.g. the views of one plugin). */
export interface GroupNode {
  kind: 'group';
  label: string;
  items: MenuItemNode[];
}

export type MenuNode = MenuItemNode | SubmenuNode | SeparatorNode | GroupNode;

export interface TopMenu {
  id: string;
  label: string;
  items: MenuNode[];
}

/** Item ids that are not commands. */
export const MENU_IDS = {
  /** `view:<view id>` toggles a view. */
  view: 'view:',
  /** `win:<label>` focuses an app window. */
  window: 'win:',
  /** `popout:<view id>` brings a popped-out panel's window to the front. */
  popout: 'popout:',
  /** Opens the searchable view picker. */
  otherWindows: 'menu.otherWindows',
  /** Shows the sidebar and focuses the fields filter. */
  fields: 'menu.fields',
  /** `recent:file:<path>` opens a recent file or folder. */
  recentFile: 'recent:file:',
  /** `recent:project:<path>` opens a recent project. */
  recentProject: 'recent:project:',
  /** Empties both recent lists. */
  recentClear: 'menu.recentClear',
} as const;

/**
 * Commands that live in the macOS app menu (Debrief ▸) in native mode instead of Help and Tools: the `app` menu of the
 * model, which the shell merges with the standard items (Services, Hide, Quit).
 */
export const APP_MENU_COMMANDS = ['help.about', 'help.checkForUpdates', 'settings.open'] as const;

export interface MenuCommand {
  id: string;
  title: string;
  keybinding?: string;
}

export interface MenuView {
  id: string;
  title: string;
  category?: ViewCategory;
  plugin?: string;
  menu?: 'palette';
  open: boolean;
}

export interface MenuWindow {
  label: string;
  title: string;
  current: boolean;
}

export interface MenuPopout {
  id: string;
  title: string;
}

/** Everything the menus depend on. Commands and views are only the AVAILABLE ones. */
export interface MenuState {
  commands: ReadonlyMap<string, MenuCommand>;
  views: readonly MenuView[];
  sidebarOpen: boolean;
  /** View › Theme: the look and the colour scheme. */
  theme: ThemeId | null;
  scheme: 'system' | 'dark' | 'light';
  /** What the source colour marker stands for (View › Source colours); default by file. */
  sourceColors?: 'source' | 'instance' | 'off';
  /** Id of the active perspective (Window ▸ Perspectives checks its command). */
  activePerspective?: number;
  timezone: 'utc' | 'local';
  markedOnly: boolean;
  undoLabel: string | null;
  redoLabel: string | null;
  windows: readonly MenuWindow[];
  popouts: readonly MenuPopout[];
  /** File ▸ Open recent: newest first (the model shows the first few of each). */
  recent?: { projects: readonly { path: string; name: string }[]; files: readonly string[] };
  /** macOS native menu bar: About and Settings are in the app menu, not in Help/Tools. */
  nativeAppMenu: boolean;
  /** Where commands sit (the registry's `menus` contributions); Debrief's own placements when left out. */
  items?: readonly DebriefMenuItemDef[];
}

/** Order of the core views at the top of the View menu (after Explorer and Fields). */
export const CORE_VIEW_ORDER = ['inspector', 'timeline', 'events', 'metrics', 'query', 'home', 'profile'];

/** Order of the View submenus. */
export const VIEW_SUBMENUS: readonly Exclude<ViewCategory, 'core'>[] = ['Analysis', 'Live', 'Profiling', 'Tools', 'Sharing', 'Diagnostics', 'Plugins', 'AI'];

/** Views listed first in their submenu, in this order; the others follow after a separator. */
const SUBMENU_ORDER: Partial<Record<ViewCategory, readonly string[]>> = {
  Analysis: ANALYSIS_MENU_ORDER,
};

/** A submenu's views: the ones with a set order first, then the rest (in registration order). */
function ordered(category: ViewCategory, views: readonly MenuView[]): Part[] {
  const order = SUBMENU_ORDER[category];
  if (!order) return views.map(viewItem);
  const first = order.map((id) => views.find((v) => v.id === id)).filter((v): v is MenuView => !!v);
  return [...first.map(viewItem), SEP, ...views.filter((v) => !first.includes(v)).map(viewItem)];
}

/** Menu labels that differ from the view titles (the tab keeps its short title). */
const VIEW_LABELS: Readonly<Record<string, string>> = {
  profile: $localize`:@@shell.menu.view.profileEditor:Profile editor`,
  plugins: $localize`:@@shell.menu.view.managePlugins:Manage plugins`,
  'ai-chat': $localize`:@@shell.menu.view.chat:Chat`,
};

/** Display names of the View submenus (the category itself stays the key). */
const CATEGORY_LABELS: Readonly<Record<Exclude<ViewCategory, 'core'>, string>> = {
  Analysis: $localize`:View submenu@@shell.menu.view.category.analysis:Analysis`,
  Live: $localize`:View submenu@@shell.menu.view.category.live:Live`,
  Profiling: $localize`:View submenu@@shell.menu.view.category.profiling:Profiling`,
  Tools: $localize`:View submenu@@shell.menu.view.category.tools:Tools`,
  Sharing: $localize`:View submenu@@shell.menu.view.category.sharing:Sharing`,
  Diagnostics: $localize`:View submenu@@shell.menu.view.category.diagnostics:Diagnostics`,
  Plugins: $localize`:View submenu@@shell.menu.view.category.plugins:Plugins`,
  AI: $localize`:View submenu@@shell.menu.view.category.ai:AI`,
};

/** How many recent projects and files File ▸ Open recent lists. */
export const RECENT_MENU_PROJECTS = 5;
export const RECENT_MENU_FILES = 10;

/** The last two path segments, e.g. "web01/orders.clef": what a recent item is called in a menu (the path is its tooltip elsewhere). */
export function recentLabel(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).slice(-2).join('/') || path;
}

/** A separator; collapsed when leading, trailing or doubled. */
const SEP: SeparatorNode = { kind: 'separator' };

type Part = MenuNode | null | undefined | false;

/** Drops missing parts, empty submenus and groups, and leading, trailing and doubled separators. */
export function tidy(parts: readonly Part[]): MenuNode[] {
  const out: MenuNode[] = [];
  for (const p of parts) {
    if (!p) continue;
    if (p.kind === 'separator') {
      if (out.length && out[out.length - 1]?.kind !== 'separator') out.push(p);
    } else if (p.kind === 'submenu') {
      const items = tidy(p.items);
      if (items.length) out.push({ ...p, items });
    } else if (p.kind === 'group') {
      if (p.items.length) out.push(p);
    } else out.push(p);
  }
  while (out.length && out[out.length - 1]?.kind === 'separator') out.pop();
  return out;
}

/** File ▸ Open recent ▸: projects, then folders and files, then Clear recent. Missing when nothing was opened yet. */
function recentMenu(state: MenuState): SubmenuNode | null {
  const projects = (state.recent?.projects ?? []).slice(0, RECENT_MENU_PROJECTS);
  const files = (state.recent?.files ?? []).slice(0, RECENT_MENU_FILES);
  if (!projects.length && !files.length) return null;
  return {
    kind: 'submenu',
    id: 'file.openRecent',
    label: $localize`:@@shell.menu.file.openRecent:Open recent`,
    items: tidy([
      ...projects.map((p): MenuItemNode => ({ kind: 'item', id: MENU_IDS.recentProject + p.path, label: p.name })),
      projects.length && files.length ? SEP : null,
      ...files.map((f): MenuItemNode => ({ kind: 'item', id: MENU_IDS.recentFile + f, label: recentLabel(f) })),
      SEP,
      { kind: 'item', id: MENU_IDS.recentClear, label: $localize`:@@shell.menu.file.clearRecent:Clear recent` },
    ]),
  };
}

/**
 * What a menu holds, top to bottom: the registered commands of a group (by `order`), a separator, a submenu (its own
 * placements under `db.<menu>.<name>`), or a part that is not a command (views, recent files, windows). Separators
 * collapse where a part is empty.
 */
type LayoutPart = 'sep' | { group: string } | { submenu: string; label: string; place: string; parts: readonly LayoutPart[] } | { slot: (state: MenuState) => Part[] };

const SEP_PART = 'sep';
const group = (name: string): LayoutPart => ({ group: name });
const submenu = (id: string, label: string, place: string, parts: readonly LayoutPart[]): LayoutPart => ({ submenu: id, label, place, parts });
const slot = (fn: (state: MenuState) => Part[]): LayoutPart => ({ slot: fn });

/** One top-level menu: its node id and label, where its commands are placed (`menu` of a placement), and its layout. */
interface TopLayout {
  id: string;
  label: string;
  place: string;
  parts: readonly LayoutPart[];
}

/** The menu bar's layout (Debrief's until M2c); the commands in it come from the placements. */
function layout(): readonly TopLayout[] {
  return [
    { id: 'app', label: 'Debrief', place: 'app', parts: [group('about'), SEP_PART, group('settings'), SEP_PART, group('quit')] },
    {
      id: 'file',
      label: $localize`:Top-level menu@@shell.menu.top.file:File`,
      place: 'file',
      parts: [
        group('new'),
        SEP_PART,
        group('open'),
        slot((s) => [recentMenu(s)]),
        SEP_PART,
        group('save'),
        SEP_PART,
        group('reimport'),
        SEP_PART,
        group('export'),
        SEP_PART,
        group('close'),
        SEP_PART,
        group('quit'),
      ],
    },
    { id: 'edit', label: $localize`:Top-level menu@@shell.menu.top.edit:Edit`, place: 'edit', parts: [group('undo'), SEP_PART, group('copy'), SEP_PART, group('paste'), SEP_PART, group('find')] },
    {
      id: 'view',
      label: $localize`:Top-level menu@@shell.menu.top.view:View`,
      place: 'view',
      parts: [
        group('sidebar'),
        slot(coreViews),
        group('new'),
        SEP_PART,
        ...VIEW_SUBMENUS.map((c) => viewSubmenu(c)),
        SEP_PART,
        slot(() => [{ kind: 'item', id: MENU_IDS.otherWindows, label: $localize`:@@shell.menu.view.otherWindows:Other windows…` }]),
        SEP_PART,
        submenu('view.theme', $localize`:@@shell.menu.view.theme:Theme`, 'db.view.theme', [group('theme'), SEP_PART, group('scheme')]),
        submenu('view.sourceColors', $localize`:@@shell.menu.view.sourceColors:Source colours`, 'db.view.source-colors', [group('mode')]),
      ],
    },
    {
      id: 'analyze',
      label: $localize`:Top-level menu@@shell.menu.top.analyze:Analyze`,
      place: 'db.analyze',
      parts: [
        group('filter'),
        SEP_PART,
        group('range'),
        submenu('analyze.timezone', $localize`:@@shell.menu.analyze.timeZone:Time zone`, 'db.analyze.time-zone', [group('zone')]),
        SEP_PART,
        group('moment'),
        submenu('analyze.compare', $localize`:Menu@@shell.menu.analyze.compare:Compare time ranges`, 'db.analyze.compare', [group('compare')]),
        group('moment-after'),
        SEP_PART,
        submenu('analyze.ai', $localize`:@@shell.menu.analyze.ai:AI`, 'db.analyze.ai', [group('ai')]),
        SEP_PART,
        group('live'),
        SEP_PART,
        group('report'),
        submenu('analyze.report', $localize`:@@shell.menu.analyze.addToReport:Add to report`, 'db.analyze.report', [group('summary'), SEP_PART, group('parts')]),
      ],
    },
    {
      id: 'tools',
      label: $localize`:Top-level menu@@shell.menu.top.tools:Tools`,
      place: 'db.tools',
      parts: [group('capture'), SEP_PART, group('parsers'), SEP_PART, group('elevate'), SEP_PART, group('settings')],
    },
    {
      id: 'window',
      label: $localize`:Top-level menu@@shell.menu.top.window:Window`,
      place: 'window',
      parts: [
        submenu('window.perspectives', $localize`:@@shell.menu.window.perspectives:Perspectives`, 'db.window.perspectives', [group('switch'), SEP_PART, group('save'), SEP_PART, group('manage')]),
        SEP_PART,
        group('tool-windows'),
        SEP_PART,
        group('panel'),
        SEP_PART,
        slot(windowGroups),
      ],
    },
    {
      id: 'help',
      label: $localize`:Top-level menu@@shell.menu.top.help:Help`,
      place: 'help',
      parts: [group('start'), SEP_PART, group('docs'), SEP_PART, group('trouble'), SEP_PART, group('about')],
    },
  ];
}

/** Builds the menu bar. Pure: same state, same menus. */
export function buildMenus(state: MenuState): TopMenu[] {
  const placements = state.items ?? DEBRIEF_MENU_ITEMS;
  const named = new Set<string>();
  const top = layout().filter((m) => m.id !== 'app' || state.nativeAppMenu);
  for (const m of top) collectGroups(m.place, m.parts, named);
  const render = (place: string, parts: readonly LayoutPart[]): Part[] =>
    parts.flatMap((p): Part[] => {
      if (p === SEP_PART) return [SEP];
      if ('group' in p) return placed(state, placements, place, p.group);
      if ('submenu' in p) return [{ kind: 'submenu', id: p.submenu, label: p.label, items: tidy([...render(p.place, p.parts), ...unnamed(state, placements, p.place, named)]) }];
      return p.slot(state);
    });
  const menus: TopMenu[] = top.map((m) => ({ id: m.id, label: m.label, items: tidy([...render(m.place, m.parts), ...unnamed(state, placements, m.place, named)]) }));
  return menus.filter((m) => m.items.length);
}

/** The `<menu>/<group>` keys a layout names. */
function collectGroups(place: string, parts: readonly LayoutPart[], into: Set<string>): void {
  for (const p of parts) {
    if (p === SEP_PART || 'slot' in p) continue;
    if ('group' in p) into.add(`${place}/${p.group}`);
    else collectGroups(p.place, p.parts, into);
  }
}

/** The commands placed in one group of a menu, by order (registration order breaks ties); unavailable ones are left out. */
function placed(state: MenuState, placements: readonly DebriefMenuItemDef[], menu: string, groupName: string): MenuItemNode[] {
  return placements
    .map((p, index) => ({ p, index }))
    .filter(({ p }) => p.menu === menu && p.group === groupName && !(p.native === 'notNative' && state.nativeAppMenu))
    .sort((a, b) => a.p.order - b.p.order || a.index - b.index)
    .flatMap(({ p }) => {
      const node = menuItem(state, p);
      return node ? [node] : [];
    });
}

/** Placements in groups the layout does not name (a plugin's): after a separator at the end of their menu, grouped. */
function unnamed(state: MenuState, placements: readonly DebriefMenuItemDef[], menu: string, named: ReadonlySet<string>): Part[] {
  const groups = new Map<string, number>();
  for (const p of placements) {
    if (p.menu !== menu || named.has(`${menu}/${p.group}`)) continue;
    groups.set(p.group, Math.min(groups.get(p.group) ?? Infinity, p.order));
  }
  return [...groups.entries()].sort((a, b) => a[1] - b[1]).flatMap(([g]) => [SEP, ...placed(state, placements, menu, g)]);
}

/** A placement as a menu item, or null when its command is not available. */
function menuItem(state: MenuState, p: DebriefMenuItemDef): MenuItemNode | null {
  const c = state.commands.get(p.command);
  if (!c) return null;
  const label = typeof p.label === 'function' ? p.label(state) : (p.label ?? c.title);
  const checked = p.checked?.(state);
  return { kind: 'item', id: p.command, label, ...(c.keybinding ? { keybinding: c.keybinding } : {}), ...(checked === undefined ? {} : { checked }) };
}

function viewItem(v: MenuView): MenuItemNode {
  return { kind: 'item', id: MENU_IDS.view + v.id, label: VIEW_LABELS[v.id] ?? v.title, checked: v.open };
}

/** The views listed in the View menu (not the palette-only ones). */
const listedViews = (state: MenuState) => state.views.filter((v) => v.menu !== 'palette');

/** Fields, then the core tool windows (the known ones in their order, then the others). */
function coreViews(state: MenuState): Part[] {
  const listed = listedViews(state);
  const core = CORE_VIEW_ORDER.map((id) => listed.find((v) => v.id === id && v.category === 'core')).filter((v): v is MenuView => !!v);
  const extraCore = listed.filter((v) => v.category === 'core' && !CORE_VIEW_ORDER.includes(v.id));
  return [{ kind: 'item', id: MENU_IDS.fields, label: $localize`:@@shell.menu.view.fields:Fields` }, ...[...core, ...extraCore].map(viewItem)];
}

/** One View submenu per category; Live starts with the switch to follow files live (F5), AI ends with Connect to Pitwall. */
function viewSubmenu(c: Exclude<ViewCategory, 'core'>): LayoutPart {
  const id = `view.${c.toLowerCase()}`;
  const inCategory = (state: MenuState) => listedViews(state).filter((v) => (v.category ?? 'Plugins') === c);
  if (c === 'Plugins') return submenu(id, CATEGORY_LABELS[c], 'db.view.plugins', [slot((s) => pluginItems(inCategory(s)))]);
  if (c === 'Live') return submenu(id, CATEGORY_LABELS[c], 'db.view.live', [group('live'), SEP_PART, slot((s) => ordered(c, inCategory(s)))]);
  if (c === 'AI') return submenu(id, CATEGORY_LABELS[c], 'db.view.ai', [slot((s) => inCategory(s).map(viewItem)), group('setup')]);
  return submenu(id, CATEGORY_LABELS[c], `db.view.${c.toLowerCase()}`, [slot((s) => ordered(c, inCategory(s)))]);
}

/** The plugin manager, then the views of each plugin under the plugin's name (only labelled when several). */
function pluginItems(views: readonly MenuView[]): MenuNode[] {
  const manager = views.filter((v) => v.category === 'Plugins' && !v.plugin);
  const byPlugin = new Map<string, MenuView[]>();
  for (const v of views) {
    if (manager.includes(v)) continue;
    const name = v.plugin ?? $localize`:@@shell.menu.view.otherPlugins:Other plugins`;
    byPlugin.set(name, [...(byPlugin.get(name) ?? []), v]);
  }
  const groups: MenuNode[] = [...byPlugin.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([label, vs]) => ({ kind: 'group', label, items: vs.map(viewItem) }));
  return tidy([...manager.map(viewItem), SEP, ...groups]);
}

/** The open windows and pop-outs (Window ▸). */
function windowGroups(state: MenuState): Part[] {
  const windows: MenuItemNode[] = state.windows.map((w) => ({ kind: 'item', id: MENU_IDS.window + w.label, label: w.title || w.label, checked: w.current }));
  const popouts: MenuItemNode[] = state.popouts.map((p) => ({ kind: 'item', id: MENU_IDS.popout + p.id, label: p.title }));
  return [
    windows.length > 1 || popouts.length ? { kind: 'group', label: $localize`:Menu group@@shell.menu.window.windows:Windows`, items: windows } : null,
    popouts.length ? { kind: 'group', label: $localize`:@@shell.menu.window.popoutPanels:Pop-out panels`, items: popouts } : null,
  ];
}

/** Every item in a menu tree, depth first. */
export function menuItems(nodes: readonly MenuNode[]): MenuItemNode[] {
  return nodes.flatMap((n) => (n.kind === 'item' ? [n] : n.kind === 'submenu' || n.kind === 'group' ? menuItems(n.items) : []));
}

/** Number of entries at the top level of a menu (a group counts its items and its label). */
export function topLevelSize(nodes: readonly MenuNode[]): number {
  return nodes.reduce((n, node) => n + (node.kind === 'group' ? node.items.length + 1 : 1), 0);
}
