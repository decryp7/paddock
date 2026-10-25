// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-storage --all-features --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const STORAGE_COMMANDS = {
  storage_cancel: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  storage_clear: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  storage_last: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
  storage_reveal: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  storage_scan: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  storage_tree: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type StorageCommand = keyof typeof STORAGE_COMMANDS;
