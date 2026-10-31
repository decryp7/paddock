// Generated from the dialog table of the app (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test it.

/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], { title, result, load })`. */
export const DIALOG_CONTRACTS = {
  'pc.config-diff': {
    id: 'pc.config-diff',
    kind: 'form',
    modality: 'modal',
    width: 720,
    widthStep: 'wide',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: [],
  },
  'pc.register': {
    id: 'pc.register',
    kind: 'form',
    modality: 'modal',
    width: 720,
    widthStep: 'wide',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: ['connection_state', 'job_cancel', 'job_result', 'jobs_snapshot', 'platform_name', 'register_runner', 'runner_status'],
  },
  'pd.settings': {
    id: 'pd.settings',
    kind: 'tool',
    modality: 'modeless',
    width: 920,
    widthStep: null,
    height: 680,
    instanced: false,
    requiredChoice: false,
    commands: ['channel_secret_clear', 'channel_secret_present', 'channel_secret_set', 'channel_test', 'clear_api_token', 'connection_state', 'dialog_open', 'export_diagnostics', 'get_autostart', 'get_settings', 'has_api_token', 'install_service', 'notification_channels_get', 'notification_channels_save', 'open_page', 'open_url', 'pitcrew_open_login_items', 'platform_name', 'repair_service', 'runner_status', 'save_settings', 'service_install_state', 'set_api_token', 'set_autostart', 'settings_*', 'uninstall_service'],
  },
} as const;

export type DialogId = keyof typeof DIALOG_CONTRACTS;
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];

/** The commands every dialog window of this owner's app may call besides the shell's base set. */
export const DIALOG_BASE = ['window_pong', 'window_ready'] as const;
