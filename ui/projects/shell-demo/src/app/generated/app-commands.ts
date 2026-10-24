// Stands in for the file paddock-commands writes from an app's command table (crates/README.md): the shell demo has no
// Rust side, so this lists the commands its mock backend answers, in the generated shape. Dialog contracts are checked
// against it (`dialogTableProblems`, `paddock-check-dialog-commands`).

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const APP_COMMANDS = {
  demo_reset: { kind: 'mutation', windows: ['main'], limit: 262144 },
  demo_status: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
  dialog_close: { kind: 'mutation', windows: ['dialog'], limit: 2097152 },
  dialog_fit_height: { kind: 'quiet', windows: ['dialog'], limit: 262144 },
  dialog_open: { kind: 'mutation', windows: ['main', 'dialog'], limit: 9437184 },
  dialog_take_input: { kind: 'query', windows: ['dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type AppCommand = keyof typeof APP_COMMANDS;
