// Mirrors the command table of paddock-channels' Tauri adapter (hand-written until the adapter generates it).

/** Every command: its kind, the window classes that may call it and its largest request in bytes. */
export const CHANNELS_COMMANDS = {
  channels_get: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  channels_save: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  channels_set_secret: { kind: 'mutation', windows: ['dialog'], limit: 65536 },
  channels_clear_secret: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  channels_test: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type ChannelsCommand = keyof typeof CHANNELS_COMMANDS;
