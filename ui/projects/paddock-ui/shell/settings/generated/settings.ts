// Generated from the Rust wire types of paddock-settings (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-settings --all-features --lib generated.

/** A stored value: durations are milliseconds, sizes are bytes. */
export type SettingValue = boolean | number | string | string[];

export type SectionOwner = "shell" | "app";

export type EnumOption = { 
/**
 * The stored value.
 */
value: string, 
/**
 * Text shown to the user.
 */
label: string, };

export type FieldKind = { "type": "bool" } | { "type": "int", min: number, max: number, 
/**
 * Granularity; at least 1.
 */
step: number, 
/**
 * Unit label shown beside the value (`"lines"`).
 */
unit: string | null, } | { "type": "float", min: number, max: number, 
/**
 * Greater than 0.
 */
step: number, unit: string | null, } | { "type": "text", 
/**
 * 1 to [`MAX_TEXT_LEN`].
 */
maxLen: number, 
/**
 * A regular expression (Rust `regex` syntax) the whole text must match.
 */
pattern: string | null, } | { "type": "enum", 
/**
 * At least one, with unique values.
 */
options: Array<EnumOption>, } | { "type": "path", dir: boolean, } | { "type": "duration", minMs: number, maxMs: number, } | { "type": "size", minBytes: number, maxBytes: number, } | { "type": "stringList", 
/**
 * 1 to [`MAX_LIST_ITEMS`].
 */
maxItems: number, };

export type SettingField = { 
/**
 * Stable key within the section (`[A-Za-z0-9._-]`); the key in the settings file.
 */
key: string, 
/**
 * Name shown to the user.
 */
label: string, 
/**
 * Help text shown under the control.
 */
description: string, kind: FieldKind, 
/**
 * The value until the user changes it; it passes the field's own validation.
 */
default: SettingValue, 
/**
 * A change takes effect only after the app restarts.
 */
restartRequired: boolean, 
/**
 * Hidden unless the user shows advanced settings.
 */
advanced: boolean, };

export type SettingsSection = { 
/**
 * Stable id (`pd.appearance`, `pw.models`); the key in the settings file.
 */
id: string, 
/**
 * Title shown for the section.
 */
title: string, 
/**
 * Help text shown under the title.
 */
description: string, owner: SectionOwner, 
/**
 * The fields, in display order.
 */
fields: Array<SettingField>, };

export type FieldError = { key: string, 
/**
 * What is wrong, for the user.
 */
message: string, };

export type LoadProblemKind = "damaged" | "newer" | "unreadable";

export type LoadProblem = { kind: LoadProblemKind, 
/**
 * For the user: what happened and what was done about it.
 */
message: string, 
/**
 * The file name the damaged or newer file was kept as, if it was moved.
 */
keptAs: string | null, };

export type SectionView = { 
/**
 * Every field present.
 */
values: Record<string, SettingValue>, 
/**
 * Stable id (`pd.appearance`, `pw.models`); the key in the settings file.
 */
id: string, 
/**
 * Title shown for the section.
 */
title: string, 
/**
 * Help text shown under the title.
 */
description: string, owner: SectionOwner, 
/**
 * The fields, in display order.
 */
fields: Array<SettingField>, };

export type SettingsOverview = { 
/**
 * In registration order.
 */
sections: Array<SectionView>, 
/**
 * A change made in this run takes effect after a restart.
 */
restartPending: boolean, 
/**
 * Why the settings file was not used at start-up, if it was not.
 */
problem: LoadProblem | null, };

export type SetResult = { 
/**
 * False when validation refused the values (see `errors`); nothing changed then.
 */
applied: boolean, 
/**
 * The change needs a restart to take effect.
 */
restartRequired: boolean, 
/**
 * One per refused field (`key` empty: the whole set).
 */
errors: Array<FieldError>, 
/**
 * The section's values now.
 */
values: Record<string, SettingValue>, };

export type SettingsChanged = { section: string, 
/**
 * The section's values now, every field present.
 */
values: Record<string, SettingValue>, 
/**
 * A change made in this run takes effect after a restart.
 */
restartPending: boolean, };

export type SettingsErrorCode = "unknownSection" | "unknownField" | "applyFailed" | "saveFailed" | "notSetUp";

export type SettingsCommandError = { code: SettingsErrorCode, message: string, retryable: boolean, };

/** Every window hears it when a change took effect; the payload is a `SettingsChanged`. */
export const SETTINGS_EVENT = 'paddock://settings';

/** The id of the shell's Appearance section and page. */
export const APPEARANCE_SECTION = 'pd.appearance';

/** The limits the schema check enforces. */
export const SETTINGS_LIMITS = { textLen: 65536, listItems: 1000, itemLen: 4096, pathLen: 4096, fields: 200, idLen: 128 } as const;
