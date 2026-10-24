// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-packs --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const PACKS_COMMANDS = {
  packs_reload: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  packs_status: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type PacksCommand = keyof typeof PACKS_COMMANDS;
