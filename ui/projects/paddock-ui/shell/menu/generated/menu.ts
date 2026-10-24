// Generated from the Rust wire types of paddock-menu (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-menu --lib generated.

export type NativeMenuEntry = { "kind": "item", id: string, label: string, 
/**
 * The key binding in the shell's syntax (`Mod+Shift+O`); the shell turns it into an accelerator.
 */
keybinding: string | null, 
/**
 * `null` for a plain item; true or false for a toggle with a check mark.
 */
checked: boolean | null, enabled: boolean, } | { "kind": "separator" } | { "kind": "header", label: string, } | { "kind": "submenu", label: string, items: Array<NativeMenuEntry>, };

export type NativeTopMenu = { id: string, label: string, items: Array<NativeMenuEntry>, };

export type NativeMenuModel = { menus: Array<NativeTopMenu>, };

export type MenuApplied = "unchanged" | "states" | "rebuilt" | "notFocused" | "inApp";

export type MenuSetErrorCode = "invalid" | "native";

export type MenuSetError = { code: MenuSetErrorCode, message: string, };

/** The command that sends a window's menus to the native menu bar. */
export const MENU_SET_COMMAND = 'menu_set';

/** The event a native menu click arrives as (to the focused top-level window); the payload is the item id. */
export const MENU_EVENT = 'paddock://menu';

/** The id of the shell's Quit command (last in the macOS app menu). */
export const QUIT_COMMAND = 'pd.app.quit';

/** The limits `menu_set` enforces. */
export const MENU_LIMITS = { menus: 16, entries: 1000, depth: 4, id: 128, label: 120 } as const;
