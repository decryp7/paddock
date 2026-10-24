// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-startup --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const STARTUP_COMMANDS = {
  app_relaunch: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  recovery_restore: { kind: 'mutation', windows: ['main'], limit: 262144 },
  recovery_safe_mode: { kind: 'mutation', windows: ['main'], limit: 262144 },
  recovery_start_fresh: { kind: 'mutation', windows: ['main'], limit: 262144 },
  recovery_status: { kind: 'query', windows: ['main'], limit: 262144 },
  ui_checkpoint: { kind: 'quiet', windows: ['main'], limit: 16777216 },
  window_boot_error: { kind: 'quiet', windows: ['main', 'popout', 'dialog'], limit: 16384 },
  window_pong: { kind: 'quiet', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  window_ready: { kind: 'quiet', windows: ['main', 'popout', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type StartupCommand = keyof typeof STARTUP_COMMANDS;
