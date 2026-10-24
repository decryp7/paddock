import type { ShellContext } from './context';
import type { ComponentLoader, LazyPart, ProvidersLoader } from './lazy';
import type { Message } from './message';
import type { IpcCommandName, TypeToken } from './types';

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
export interface SettingsPageDef<T extends object = object> extends LazyPart {
  /** Also the id of the settings section in Rust (`paddock-settings`), checked through the manifest. */
  readonly id: string;
  readonly group: SettingsGroup;
  readonly title: Message;
  readonly icon?: string;
  readonly order: number;
  readonly when?: Live<T, boolean>;
}

/** The dialog kinds of ADR 0009. Each fixes what the shell applies (modality, size policy, footer, keys; plan S6). */
export const DIALOG_KINDS = ['decision', 'form', 'viewer', 'tool'] as const;
export type DialogKind = (typeof DIALOG_KINDS)[number];
export type DialogModality = 'modal' | 'modeless';

/** The modalities each kind allows: one entry means the kind decides, two means the dialog declares it. */
export const KIND_MODALITIES: Readonly<Record<DialogKind, readonly DialogModality[]>> = {
  decision: ['modal'],
  form: ['modal', 'modeless'],
  viewer: ['modeless'],
  tool: ['modeless'],
};

/** What `defineDialog` takes: the whole contract of one dialog window. */
export interface DialogSpec<Id extends string, R, Cmd extends string> {
  /** Stable, namespaced id: the dialog window's route, its remembered bounds' key, its test id. */
  readonly id: Id;
  readonly kind: DialogKind;
  /** Only a `form` chooses; the other kinds decide. */
  readonly modality?: DialogModality;
  readonly title: Message;
  /** Opening size in logical px; the kind supplies the rest (plan S6). */
  readonly width?: number;
  readonly height?: number;
  /** Never smaller than this; must not exceed the opening size. */
  readonly minWidth?: number;
  readonly minHeight?: number;
  /** Each distinct input is its own window; without it, one window per id. */
  readonly instanced?: boolean;
  /** A modal without a Cancel: Escape and close requests leave it open. */
  readonly requiredChoice?: boolean;
  /** What the opener's promise resolves to. */
  readonly result?: TypeToken<R>;
  /**
   * Every IPC command the dialog's window may call beyond the kind's and the app's base set; a trailing `*` is a prefix.
   * Rust's guard allows exactly these (from the manifest).
   */
  readonly commands: readonly (Cmd | `${string}*`)[];
  /** The providers the dialog's window boots besides the shell's base set (rule 34). */
  readonly providers?: ProvidersLoader;
  /** The content component, loaded in the dialog's window only. */
  readonly load: ComponentLoader;
}

/** A defined dialog. `modality` is resolved from the kind when the kind decides (left as declared otherwise). */
export interface DialogDef<Id extends string = string, R = unknown, Cmd extends string = string> extends DialogSpec<Id, R, Cmd> {
  readonly modality: DialogModality;
}

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
 * Declares one dialog window. `Cmd` is the app's generated IPC command union once it is registered in `ShellTypes`
 * (`string` until then), so a contract naming a command that does not exist fails to compile.
 */
export function defineDialog<const Id extends string, R = void, Cmd extends string = IpcCommandName>(
  spec: DialogSpec<Id, R, Cmd>,
): DialogDef<Id, R, Cmd> {
  const allowed = KIND_MODALITIES[spec.kind] as readonly DialogModality[] | undefined;
  // Only an unambiguous kind decides; a form without a modality keeps none here and the lint reports it.
  const modality = spec.modality ?? (allowed?.length === 1 ? allowed[0] : undefined);
  return frozen({ ...spec, modality: modality as DialogModality, commands: Object.freeze([...spec.commands]) });
}

export function defineHealthService(spec: HealthServiceDef): HealthServiceDef {
  return frozen({ ...spec });
}

export function defineNotificationKind(spec: NotificationKindDef): NotificationKindDef {
  return frozen({ ...spec, actions: Object.freeze([...spec.actions]) });
}
