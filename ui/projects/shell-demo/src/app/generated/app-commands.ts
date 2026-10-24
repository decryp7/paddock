// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --test demo.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const APP_COMMANDS = {
  demo_reset: { kind: 'mutation', windows: ['main'], limit: 262144 },
  demo_status: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type AppCommand = keyof typeof APP_COMMANDS;
