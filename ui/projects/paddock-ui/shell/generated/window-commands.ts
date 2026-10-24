// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-window --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const WINDOW_COMMANDS = {
  app_quit: { kind: 'mutation', windows: ['main', 'popout', 'dialog', 'tray'], limit: 262144 },
  tray_close: { kind: 'mutation', windows: ['tray'], limit: 262144 },
  tray_fit_height: { kind: 'quiet', windows: ['tray'], limit: 262144 },
  window_close: { kind: 'mutation', windows: ['main'], limit: 262144 },
  window_focus: { kind: 'mutation', windows: ['main', 'popout', 'dialog', 'tray'], limit: 262144 },
  window_new: { kind: 'mutation', windows: ['main', 'tray'], limit: 262144 },
  window_resolve_close: { kind: 'mutation', windows: ['main'], limit: 262144 },
  window_resolve_open: { kind: 'mutation', windows: ['main'], limit: 262144 },
  window_set_title: { kind: 'quiet', windows: ['main'], limit: 4096 },
  window_take_pending: { kind: 'mutation', windows: ['main'], limit: 262144 },
  windows_list: { kind: 'query', windows: ['main', 'popout', 'dialog', 'tray'], limit: 262144 },
} as const;

/** A command name of the table. */
export type WindowCommand = keyof typeof WINDOW_COMMANDS;
