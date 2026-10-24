// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-live --features tauri --test tauri generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const LIVE_COMMANDS = {
  live_snapshot: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type LiveCommand = keyof typeof LIVE_COMMANDS;
