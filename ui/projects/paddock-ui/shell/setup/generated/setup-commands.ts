// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --lib setup::generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const SETUP_COMMANDS = {
  setup_act: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  setup_finish: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  setup_repair: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  setup_skip: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  setup_state: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type SetupCommand = keyof typeof SETUP_COMMANDS;
