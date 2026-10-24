// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-settings --all-features --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const SETTINGS_COMMANDS = {
  settings_overview: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  settings_reset: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  settings_set: { kind: 'mutation', windows: ['main', 'dialog'], limit: 1048576 },
} as const;

/** A command name of the table. */
export type SettingsCommand = keyof typeof SETTINGS_COMMANDS;
