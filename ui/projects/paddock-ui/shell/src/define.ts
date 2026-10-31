import type { DialogSize } from '../generated/dialogs';
import { DIALOG_KIND_TABLE, DIALOG_MIN_SIZE, type DialogWidthStep } from '../generated/dialog-kinds';
import type { ShellContext } from './context';
import type { ComponentLoader, LazyPart, ProvidersLoader } from './lazy';
import type { Message } from './message';
import type { TypeToken } from './types';

/*
 * The contribution kinds of the shell (ADR 0012 §2). Each `define*` returns a frozen value and validates nothing: every
 * check lives in the lint (`assertContributionsValid` in `shell/testing`), so a defect is reported for every instance at
 * once rather than thrown by the first one at start-up.
 *
 * Lazy by construction (plan §9): a contribution is static data plus loaders. Live functions (`when`, `enabled`,
 * `checked`, `badge`, `run`) take the app's context as an argument instead of importing what they need, so a
 * registration file (`*.contributions.ts`) imports no store, service or command class; components and providers are
 * `import()`ed when first shown. `checkRegistrationSource` in `shell/testing` enforces it.
 */

/** A function over the context, evaluated defensively: if it throws, that one item is hidden or keeps its default. */
export type Live<T extends object, R> = (ctx: ShellContext<T>) => R;

/** An action in menus, the palette and key bindings. */
export interface CommandDef<T extends object = object> {
  /** Stable, namespaced id (`pw.models.refresh`); the native menu item id of its menu entries. */
  readonly id: string;
  readonly title: Message;
  /** Groups commands in the palette. */
  readonly category?: Message;
  /** Name of an icon in the app's or the shell's `MatIconRegistry` namespace (`pw:model`). */
  readonly icon?: string;
  /** Offered (menus, palette, keys) only while this holds; defaults to always. */
  readonly when?: Live<T, boolean>;
  /** Shown but inert while this is false; defaults to enabled. */
  readonly enabled?: Live<T, boolean>;
  /** A check mark in menus (toggles). */
  readonly checked?: Live<T, boolean>;
  readonly run: (ctx: ShellContext<T>, args?: unknown) => void | Promise<void>;
}

/** A key that runs a command. `scope` separates bindings that are active in different places (a page, a dialog). */
export interface KeybindingDef {
  readonly command: string;
  /** `Mod+R`, `Alt+Left`, `F5` (see `parseKey`). */
  readonly key: string;
  /** Where the binding is active; bindings conflict only within one scope. Defaults to `window`. */
  readonly scope: string;
}

/** The shell's top-level menus; an app adds submenus by namespaced id (`pw.models`). */
export const SHELL_MENUS = ['app', 'file', 'edit', 'view', 'window', 'help'] as const;
export type ShellMenu = (typeof SHELL_MENUS)[number];

/** A command's entry in a menu (native and in-app, plan S13). */
export interface MenuItemDef<T extends object = object> {
  readonly menu: ShellMenu | (string & {});
  readonly command: string;
  /** Separators fall between groups; groups are ordered by their lowest `order`. */
  readonly group: string;
  /** Ascending within the menu; shell items use multiples of 100. */
  readonly order: number;
  readonly when?: Live<T, boolean>;
}

/** A page of the pages frame (plan S14): an entry in the sidebar and a routed body. */
export interface PageDef<T extends object = object> extends LazyPart {
  readonly id: string;
  readonly title: Message;
  readonly icon: string;
  /** Ascending, top to bottom; shell pages use multiples of 100. */
  readonly order: number;
  /** `bottom`: pinned at the foot of the sidebar (Settings, About). Defaults to `top`. */
  readonly section: 'top' | 'bottom';
  readonly when?: Live<T, boolean>;
  /** A short count drawn on the icon (null draws none). */
  readonly badge?: Live<T, string | number | null>;
  /** What the badge counts, as its tooltip and accessible name ("1 runner offline"); without it, the count alone. */
  readonly badgeLabel?: Live<T, string | null>;
}

/** An item of the status bar, laid out by priority when the bar is narrow (plan §5.7). */
export interface StatusItemDef<T extends object = object> extends LazyPart {
  readonly id: string;
  /** Accessible name. */
  readonly title: Message;
  readonly side: 'start' | 'end';
  /** Higher stays visible longer when the bar is narrow (0..1000). */
  readonly priority: number;
  /**
   * Never hidden for want of room (it shrinks to its compact form instead): an item that carries attention, such as the
   * notifications' unread count or the service chip (plan S14 review).
   */
  readonly pinned?: boolean;
  readonly when?: Live<T, boolean>;
}

/** Where a settings page is listed in the Settings window (plan S10). */
export type SettingsGroup = 'application' | 'features' | 'advanced';

/** A page of the Settings window. */
export interface SettingsPageDef<T extends object = object> extends Omit<LazyPart, 'load'> {
  /**
   * The page's component; without it the page is the Rust section of the same id in the shell's generic form (the
   * Settings window brings that form, so a registration names no settings UI and stays light).
   */
  readonly load?: LazyPart['load'];
  /** Also the id of the settings section in Rust (`paddock-settings`), checked through the manifest. */
  readonly id: string;
  readonly group: SettingsGroup;
  /**
   * A heading inside the group (a sub-group in the navigation, "Network" among the Features pages): pages with the same
   * sub-group sit together under it, in page order; pages without one come first, under the group heading alone.
   */
  readonly subgroup?: Message;
  readonly title: Message;
  readonly icon?: string;
  readonly order: number;
  readonly when?: Live<T, boolean>;
  /**
   * Whether a change on this page is saved as it is made (the default). A page with its own Save button declares `false`, and the
   * Settings footer then does not say "Changes are saved as you make them." while it is open.
   */
  readonly autosaves?: boolean;
}

/** The dialog kinds of ADR 0009. Each fixes what the shell applies (modality, size policy, footer, keys; ADR 0026 §1). */
export const DIALOG_KINDS = ['decision', 'form', 'viewer', 'tool'] as const;
export type DialogKind = (typeof DIALOG_KINDS)[number];
export type DialogModality = 'modal' | 'modeless';

/** The modalities each kind allows (from the generated kind table: Rust is the source). */
export const KIND_MODALITIES: Readonly<Record<DialogKind, readonly DialogModality[]>> = {
  decision: DIALOG_KIND_TABLE.decision.modalities,
  form: DIALOG_KIND_TABLE.form.modalities,
  viewer: DIALOG_KIND_TABLE.viewer.modalities,
  tool: DIALOG_KIND_TABLE.tool.modalities,
};

/**
 * One dialog's contract as Rust declares it (ADR 0026 §3): an entry of an owner's generated `DIALOG_CONTRACTS`
 * (`paddock_dialogs::dialogs!`). Kind, modality, size and commands are never written in TypeScript.
 */
export interface DialogContractData<Id extends string = string, Cmd extends string = string> {
  /** Stable, namespaced id: the dialog window's route, its remembered bounds' key, its test id. */
  readonly id: Id;
  readonly kind: DialogKind;
  readonly modality: DialogModality;
  /** The width it opens at, logical px. */
  readonly width: number;
  /** `narrow`, `medium` or `wide`; `null` for a tool's own number. */
  readonly widthStep: DialogWidthStep | null;
  /** The preferred height of a viewer or a tool, logical px; `null`: measured (a decision or a form). */
  readonly height: number | null;
  /** Each distinct input is its own window; without it, one window per id. */
  readonly instanced: boolean;
  /** A modal without a way out: no dismiss action; Escape and close requests leave it open. */
  readonly requiredChoice: boolean;
  /** Every IPC command the dialog's window may call beyond the base set; a trailing `*` is a prefix. */
  readonly commands: readonly Cmd[];
}

/** What only TypeScript knows of a dialog: its title, typed result, component and providers. `C` is the content component. */
export interface DialogUi<R, C = unknown> {
  readonly title: Message;
  /** What the opener's answer carries. */
  readonly result?: TypeToken<R>;
  /** The providers the dialog's window boots besides the shell's base set (rule 34). */
  readonly providers?: ProvidersLoader;
  /** The content component, loaded in the dialog's window only. */
  readonly load: ComponentLoader<C>;
}

/** A defined dialog: its Rust contract and its TypeScript part. */
export interface DialogDef<Id extends string = string, R = unknown, Cmd extends string = string, C = unknown>
  extends DialogContractData<Id, Cmd>,
    DialogUi<R, C> {}

/** The least a dialog window can be (`DIALOG_MIN` in `paddock-dialogs`). */
export const DIALOG_MIN: Readonly<DialogSize> = Object.freeze({ ...DIALOG_MIN_SIZE });
/** The viewer floor (`VIEWER_MIN`). */
export const VIEWER_MIN: Readonly<DialogSize> = Object.freeze({ ...DIALOG_KIND_TABLE.viewer.minSize });

/** The smallest window of each kind (the viewer floor), or none (from the generated kind table). */
export const KIND_MIN_SIZE: Readonly<Record<DialogKind, Readonly<DialogSize> | null>> = Object.freeze({
  decision: DIALOG_KIND_TABLE.decision.minSize,
  form: DIALOG_KIND_TABLE.form.minSize,
  viewer: VIEWER_MIN,
  tool: DIALOG_KIND_TABLE.tool.minSize,
});

/** A service whose health the shell shows (chip, notifications; plan S4). Rust has a supervisor of the same id. */
export interface HealthServiceDef {
  readonly id: string;
  /** What the user calls it ("the model engine"). */
  readonly label: Message;
  readonly icon: string;
}

/** A kind of notification the app raises, with the actions its notifications offer. */
export interface NotificationKindDef {
  readonly id: string;
  readonly title: Message;
  /** Action ids (`install`, `later`), in button order. */
  readonly actions: readonly string[];
}

/** Optional fields left out of a spec; `define*` fills what has a default. */
type Opt<T, K extends keyof T> = Omit<T, K> & Partial<Pick<T, K>>;

function frozen<T extends object>(value: T): T {
  return Object.freeze(value);
}

export function defineCommand<T extends object = object>(spec: CommandDef<T>): CommandDef<T> {
  return frozen({ ...spec });
}

export function defineKeybinding(spec: Opt<KeybindingDef, 'scope'>): KeybindingDef {
  return frozen({ ...spec, scope: spec.scope ?? 'window' });
}

export function defineMenuItem<T extends object = object>(spec: Opt<MenuItemDef<T>, 'group'>): MenuItemDef<T> {
  return frozen({ ...spec, group: spec.group ?? 'main' });
}

export function definePage<T extends object = object>(spec: Opt<PageDef<T>, 'section'>): PageDef<T> {
  return frozen({ ...spec, section: spec.section ?? 'top' });
}

export function defineStatusItem<T extends object = object>(spec: StatusItemDef<T>): StatusItemDef<T> {
  return frozen({ ...spec });
}

export function defineSettingsPage<T extends object = object>(spec: SettingsPageDef<T>): SettingsPageDef<T> {
  return frozen({ ...spec });
}

/**
 * Declares one dialog window from its Rust contract (`DIALOG_CONTRACTS['<id>']`, generated from the owner's `dialogs!`
 * table) and what only TypeScript knows: `defineDialog(DIALOG_CONTRACTS['pw.quit'], { title, result: type<QuitChoice>(), load })`.
 */
export function defineDialog<const Id extends string, R = void, Cmd extends string = string, C = unknown>(
  contract: DialogContractData<Id, Cmd>,
  ui: DialogUi<R, C>,
): DialogDef<Id, R, Cmd, C> {
  return frozen({ ...contract, commands: Object.freeze([...contract.commands]), ...ui });
}

export function defineHealthService(spec: HealthServiceDef): HealthServiceDef {
  return frozen({ ...spec });
}

export function defineNotificationKind(spec: NotificationKindDef): NotificationKindDef {
  return frozen({ ...spec, actions: Object.freeze([...spec.actions]) });
}
