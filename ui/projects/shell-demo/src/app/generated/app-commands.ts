// Generated from the command table by paddock-commands. Do not edit: cd apps/shell-demo/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test app.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const APP_COMMANDS = {
  demo_break: { kind: 'mutation', windows: ['main'], limit: 262144 },
  demo_job_start: { kind: 'mutation', windows: ['main', 'tray'], limit: 262144 },
  demo_reset: { kind: 'mutation', windows: ['main'], limit: 262144 },
  demo_status: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type AppCommand = keyof typeof APP_COMMANDS;
