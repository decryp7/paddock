// Generated from the dialog table of the app (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: cd apps/workbench-demo/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test app.

/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], { title, result, load })`. */
export const DIALOG_CONTRACTS = {
} as const;

export type DialogId = keyof typeof DIALOG_CONTRACTS;
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];

/** The commands every dialog window of this owner's app may call besides the shell's base set. */
export const DIALOG_BASE = ['ui_sync_publish', 'window_pong', 'window_ready'] as const;
