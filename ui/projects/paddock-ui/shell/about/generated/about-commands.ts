// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-updater --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const ABOUT_COMMANDS = {
  about_info: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  about_legal: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  about_whats_new: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  about_whats_new_start: { kind: 'mutation', windows: ['main'], limit: 262144 },
  updates_cancel: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  updates_check: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  updates_download: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  updates_install_on_quit: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  updates_restart: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  updates_set_preferences: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  updates_status: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type AboutCommand = keyof typeof ABOUT_COMMANDS;
