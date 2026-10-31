// Generated from the dialog table of the app (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell.

/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], { title, result, load })`. */
export const DIALOG_CONTRACTS = {
  'pd.settings': {
    id: 'pd.settings',
    kind: 'tool',
    modality: 'modeless',
    width: 880,
    widthStep: null,
    height: 640,
    instanced: false,
    requiredChoice: false,
    commands: ['app_info', 'choose_folder', 'edit_popover', 'get_autostart', 'get_settings', 'install_renderer', 'job_cancel', 'jobs_snapshot', 'mail_status', 'open', 'pause_notifications', 'popover_values', 'popover_view', 'renderer_status', 'save_settings', 'send_test_email', 'set_autostart', 'set_mail_password', 'settings_*'],
  },
} as const;

export type DialogId = keyof typeof DIALOG_CONTRACTS;
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];

/** The commands every dialog window of this owner's app may call besides the shell's base set. */
export const DIALOG_BASE = ['window_pong', 'window_ready'] as const;
