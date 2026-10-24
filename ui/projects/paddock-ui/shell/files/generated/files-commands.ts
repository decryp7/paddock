// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-files --all-features --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const FILES_COMMANDS = {
  files_open: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  files_open_app_folder: { kind: 'mutation', windows: ['main', 'popout', 'dialog', 'tray'], limit: 262144 },
  files_open_url: { kind: 'mutation', windows: ['main', 'popout', 'dialog', 'tray'], limit: 262144 },
  files_pick: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  files_recent_clear: { kind: 'mutation', windows: ['main', 'popout', 'dialog', 'tray'], limit: 262144 },
  files_recent_grant: { kind: 'mutation', windows: ['main', 'tray'], limit: 262144 },
  files_recent_list: { kind: 'query', windows: ['main', 'popout', 'dialog', 'tray'], limit: 262144 },
  files_recent_remove: { kind: 'mutation', windows: ['main', 'popout', 'dialog', 'tray'], limit: 262144 },
  files_reveal: { kind: 'mutation', windows: ['main', 'popout', 'dialog', 'tray'], limit: 262144 },
} as const;

/** A command name of the table. */
export type FilesCommand = keyof typeof FILES_COMMANDS;
