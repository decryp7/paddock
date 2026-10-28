// Generated from the command table by paddock-commands. Do not edit: cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const APP_COMMANDS = {
  dyno: { kind: 'mutation', windows: ['main', 'dialog', 'tray'], limit: 262144 },
  pick_folder: { kind: 'mutation', windows: ['main'], limit: 262144 },
  save_diagnostics: { kind: 'mutation', windows: ['main'], limit: 262144 },
  save_inventory_report: { kind: 'mutation', windows: ['main'], limit: 262144 },
  save_issues: { kind: 'mutation', windows: ['main'], limit: 262144 },
  save_report: { kind: 'mutation', windows: ['main'], limit: 262144 },
} as const;

/** A command name of the table. */
export type AppCommand = keyof typeof APP_COMMANDS;
