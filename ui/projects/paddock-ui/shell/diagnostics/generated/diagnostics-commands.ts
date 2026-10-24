// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-diag --all-features --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const DIAGNOSTICS_COMMANDS = {
  diagnostics_cancel: { kind: 'mutation', windows: ['dialog'], limit: 262144 },
  diagnostics_preview: { kind: 'mutation', windows: ['dialog'], limit: 262144 },
  diagnostics_save: { kind: 'mutation', windows: ['dialog'], limit: 262144 },
  log_from_webview: { kind: 'quiet', windows: ['main', 'popout', 'dialog', 'tray'], limit: 65536 },
} as const;

/** A command name of the table. */
export type DiagnosticsCommand = keyof typeof DIAGNOSTICS_COMMANDS;
