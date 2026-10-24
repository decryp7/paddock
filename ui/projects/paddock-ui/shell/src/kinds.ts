import {
  CommandDef,
  DIALOG_KINDS,
  DialogDef,
  HealthServiceDef,
  KeybindingDef,
  KIND_MODALITIES,
  MenuItemDef,
  NotificationKindDef,
  PageDef,
  SettingsPageDef,
  SHELL_MENUS,
  StatusItemDef,
} from './define';
import { pressedKeys, parseKey } from './keys';
import { CONTRIBUTION_ID, isMessage } from './message';

/** What a kind's checks may ask about the other contributions (for cross-references such as menu → command). */
export interface ContributionLookup {
  /** Whether a contribution of `kind` with this identity is registered. */
  has(kind: string, identity: string): boolean;
  items(kind: string): readonly unknown[];
}

/**
 * One kind of contribution: its batch key, its identity (duplicates are reported), and its checks for the lint. The
 * shell's kinds are `SHELL_KINDS`; the workbench adds its kinds (views, activity items, ...) to the same registry with
 * `provideContributionKinds` (plan W1), never a second mechanism.
 */
export interface ContributionKind<I = unknown> {
  /** The key in `provideContributions({...})` and the registry: `commands`. */
  readonly name: string;
  /** Singular noun used in lint messages: `command`. */
  readonly label: string;
  /** Unique per kind; two items with the same identity are a duplicate. */
  identity(item: I): string;
  /** Defects of one item, as plain sentences without the item's name. */
  check?(item: I, lookup: ContributionLookup): readonly string[];
  /** Defects across the kind's items (key conflicts), as whole sentences. */
  checkAll?(items: readonly I[], lookup: ContributionLookup): readonly string[];
}

/** Declares a kind (identity function for the types). */
export function defineContributionKind<I>(kind: ContributionKind<I>): ContributionKind<I> {
  return Object.freeze(kind);
}

const issues = (...checks: (string | false | null | undefined)[]): string[] => checks.filter((c): c is string => typeof c === 'string');

const idProblem = (id: unknown): string | false =>
  (typeof id !== 'string' || !CONTRIBUTION_ID.test(id)) && `id ${JSON.stringify(id)} is not a namespaced id (pw.models.refresh)`;
const messageProblem = (field: string, value: unknown): string | false => !isMessage(value) && `${field} has no message id (use msg('<ns>.<name>', 'Text'))`;
const iconProblem = (icon: unknown): string | false => (typeof icon !== 'string' || !/^[a-z][a-z0-9-]*:[a-z0-9][a-z0-9-]*$/.test(icon)) && `has no icon (namespace:name)`;
const loaderProblem = (value: unknown, field = 'load'): string | false => typeof value !== 'function' && `has no ${field} function`;
const optionalLoader = (value: unknown, field: string): string | false => value !== undefined && typeof value !== 'function' && `${field} is not a loader function`;
const numberProblem = (field: string, value: unknown, min: number, max: number): string | false =>
  (typeof value !== 'number' || !Number.isFinite(value) || value < min || value > max) && `${field} must be a number from ${min} to ${max}`;

export const COMMANDS = defineContributionKind<CommandDef>({
  name: 'commands',
  label: 'command',
  identity: (c) => c.id,
  check: (c) => issues(idProblem(c.id), messageProblem('title', c.title), c.category !== undefined && messageProblem('category', c.category), loaderProblem(c.run, 'run')),
});

export const KEYBINDINGS = defineContributionKind<KeybindingDef>({
  name: 'keybindings',
  label: 'keybinding',
  identity: (k) => `${k.scope}:${k.key}:${k.command}`,
  check: (k, lookup) =>
    issues(
      !parseKey(k.key) && `key ${JSON.stringify(k.key)} is not a key binding (Mod+Shift+P)`,
      !lookup.has('commands', k.command) && `names the missing command ${k.command}`,
      !/^[a-z][a-z0-9.:-]*$/.test(k.scope) && `scope ${JSON.stringify(k.scope)} is malformed`,
    ),
  checkAll: (items) => {
    const out: string[] = [];
    for (const platform of ['mac', 'other'] as const) {
      const seen = new Map<string, KeybindingDef>();
      for (const k of items) {
        const pressed = pressedKeys(k.key, platform);
        if (!pressed) continue;
        const slot = `${k.scope}|${pressed}`;
        const first = seen.get(slot);
        if (!first) seen.set(slot, k);
        else if (first.command !== k.command) {
          const msg = `keybinding conflict in scope ${k.scope}: ${first.key} (${first.command}) and ${k.key} (${k.command}) press ${pressed}${platform === 'mac' ? ' on macOS' : ' on Windows and Linux'}`;
          if (!out.includes(msg)) out.push(msg);
        }
      }
    }
    return out;
  },
});

export const MENUS = defineContributionKind<MenuItemDef>({
  name: 'menus',
  label: 'menu item',
  identity: (m) => `${m.menu}/${m.command}`,
  check: (m, lookup) =>
    issues(
      !(SHELL_MENUS as readonly string[]).includes(m.menu) && !CONTRIBUTION_ID.test(m.menu) && `menu ${JSON.stringify(m.menu)} is neither a shell menu (${SHELL_MENUS.join(', ')}) nor a namespaced submenu`,
      !lookup.has('commands', m.command) && `names the missing command ${m.command}`,
      !/^[a-z][a-z0-9-]*$/.test(m.group) && `group ${JSON.stringify(m.group)} is malformed`,
      numberProblem('order', m.order, 0, 100_000),
    ),
});

export const PAGES = defineContributionKind<PageDef>({
  name: 'pages',
  label: 'page',
  identity: (p) => p.id,
  check: (p) => issues(idProblem(p.id), messageProblem('title', p.title), iconProblem(p.icon), numberProblem('order', p.order, 0, 100_000), loaderProblem(p.load), optionalLoader(p.providers, 'providers')),
});

export const STATUS_ITEMS = defineContributionKind<StatusItemDef>({
  name: 'statusItems',
  label: 'status item',
  identity: (s) => s.id,
  check: (s) =>
    issues(
      idProblem(s.id),
      messageProblem('title', s.title),
      s.side !== 'start' && s.side !== 'end' && `side must be start or end`,
      numberProblem('priority', s.priority, 0, 1000),
      loaderProblem(s.load),
      optionalLoader(s.providers, 'providers'),
    ),
});

export const SETTINGS_PAGES = defineContributionKind<SettingsPageDef>({
  name: 'settingsPages',
  label: 'settings page',
  identity: (s) => s.id,
  check: (s) =>
    issues(
      idProblem(s.id),
      messageProblem('title', s.title),
      !['application', 'features', 'advanced'].includes(s.group) && `group must be application, features or advanced`,
      s.icon !== undefined && iconProblem(s.icon),
      numberProblem('order', s.order, 0, 100_000),
      loaderProblem(s.load),
      optionalLoader(s.providers, 'providers'),
    ),
});

const sizeProblem = (field: string, value: unknown): string | false => value !== undefined && numberProblem(field, value, 1, 10_000);

export const DIALOGS = defineContributionKind<DialogDef>({
  name: 'dialogs',
  label: 'dialog',
  identity: (d) => d.id,
  check: (d) => {
    const kindKnown = (DIALOG_KINDS as readonly string[]).includes(d.kind);
    const allowed = kindKnown ? KIND_MODALITIES[d.kind] : [];
    return issues(
      idProblem(d.id),
      messageProblem('title', d.title),
      !kindKnown && (d.kind === undefined ? `has no kind (${DIALOG_KINDS.join(', ')})` : `kind ${JSON.stringify(d.kind)} is not one of ${DIALOG_KINDS.join(', ')}`),
      kindKnown && d.modality === undefined && `a ${d.kind} dialog declares its modality (${allowed.join(' or ')})`,
      kindKnown && d.modality !== undefined && !allowed.includes(d.modality) && `a ${d.kind} dialog cannot be ${d.modality}`,
      d.requiredChoice === true && d.modality !== 'modal' && `only a modal can be a required choice`,
      sizeProblem('width', d.width),
      sizeProblem('height', d.height),
      sizeProblem('minWidth', d.minWidth),
      sizeProblem('minHeight', d.minHeight),
      d.minWidth !== undefined && d.width !== undefined && d.minWidth > d.width && `layout min > pref: minWidth ${d.minWidth} exceeds width ${d.width}`,
      d.minHeight !== undefined && d.height !== undefined && d.minHeight > d.height && `layout min > pref: minHeight ${d.minHeight} exceeds height ${d.height}`,
      !Array.isArray(d.commands) && `has no commands list (an empty list is fine)`,
      ...(Array.isArray(d.commands) ? d.commands.map((c) => !/^(?:plugin:[a-z0-9-]+\|)?[a-z][a-z0-9_]*\*?$/.test(c) && `command ${JSON.stringify(c)} is not an IPC command name`) : []),
      loaderProblem(d.load),
      optionalLoader(d.providers, 'providers'),
    );
  },
});

export const HEALTH_SERVICES = defineContributionKind<HealthServiceDef>({
  name: 'healthServices',
  label: 'health service',
  identity: (h) => h.id,
  check: (h) => issues(idProblem(h.id), messageProblem('label', h.label), iconProblem(h.icon)),
});

export const NOTIFICATION_KINDS = defineContributionKind<NotificationKindDef>({
  name: 'notificationKinds',
  label: 'notification kind',
  identity: (n) => n.id,
  check: (n) =>
    issues(
      idProblem(n.id),
      messageProblem('title', n.title),
      !Array.isArray(n.actions) && `has no actions list`,
      ...(Array.isArray(n.actions) ? n.actions.map((a) => !/^[a-z][a-z0-9-]*$/.test(a) && `action ${JSON.stringify(a)} is malformed`) : []),
      Array.isArray(n.actions) && new Set(n.actions).size !== n.actions.length && `lists an action twice`,
    ),
});

/** The shell's contribution kinds, in registry order. */
export const SHELL_KINDS: readonly ContributionKind[] = [
  COMMANDS,
  KEYBINDINGS,
  MENUS,
  PAGES,
  STATUS_ITEMS,
  SETTINGS_PAGES,
  DIALOGS,
  HEALTH_SERVICES,
  NOTIFICATION_KINDS,
] as readonly ContributionKind<never>[] as readonly ContributionKind[];
