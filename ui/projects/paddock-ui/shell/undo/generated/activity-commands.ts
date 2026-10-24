// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-jobs --features tauri --test it tauri_activity::generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const ACTIVITY_COMMANDS = {
  activity_list: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  activity_mark: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  activity_undo: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type ActivityCommandName = keyof typeof ACTIVITY_COMMANDS;
