// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-channels --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const CHANNELS_COMMANDS = {
  channels_clear_secret: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  channels_get: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  channels_save: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  channels_set_secret: { kind: 'mutation', windows: ['dialog'], limit: 262144 },
  channels_test: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type ChannelsCommand = keyof typeof CHANNELS_COMMANDS;
