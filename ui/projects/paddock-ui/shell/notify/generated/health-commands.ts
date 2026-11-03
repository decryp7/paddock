// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-health --all-features --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const HEALTH_COMMANDS = {
  health_repair: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  health_snapshot: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  notifications_dismiss: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  notifications_dismiss_all: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  notifications_dismiss_many: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  notifications_list: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  notifications_mark_all_read: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  notifications_mark_read: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  notifications_post: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 16384 },
  notifications_record: { kind: 'quiet', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  notifications_restore: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 4194304 },
} as const;

/** A command name of the table. */
export type HealthCommand = keyof typeof HEALTH_COMMANDS;
