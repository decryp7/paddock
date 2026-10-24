// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-assistant --all-features --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const ASSISTANT_COMMANDS = {
  assistant_approve_tool: { kind: 'mutation', windows: ['main', 'popout'], limit: 262144 },
  assistant_conversations: { kind: 'query', windows: ['main', 'popout'], limit: 262144 },
  assistant_delete: { kind: 'mutation', windows: ['main', 'popout'], limit: 262144 },
  assistant_info: { kind: 'query', windows: ['main', 'popout'], limit: 262144 },
  assistant_load: { kind: 'query', windows: ['main', 'popout'], limit: 262144 },
  assistant_send: { kind: 'mutation', windows: ['main', 'popout'], limit: 1048576 },
  assistant_stop: { kind: 'mutation', windows: ['main', 'popout'], limit: 262144 },
} as const;

/** A command name of the table. */
export type AssistantCommand = keyof typeof ASSISTANT_COMMANDS;
