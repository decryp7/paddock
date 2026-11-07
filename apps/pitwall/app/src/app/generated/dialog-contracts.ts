// Generated from the dialog table of Pitwall (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell.

/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], { title, result, load })`. */
export const DIALOG_CONTRACTS = {
  'pd.settings': {
    id: 'pd.settings',
    kind: 'tool',
    modality: 'modeless',
    width: 880,
    widthStep: null,
    height: 700,
    instanced: false,
    requiredChoice: false,
    commands: ['app_job_cancel', 'app_job_stop_waiting', 'apply_preset', 'cache_info', 'clear_cache', 'connect_app', 'connected_apps', 'decline_service', 'dialog_open', 'edit_popover', 'forget_app', 'get_autostart', 'open_service_login_items', 'overview', 'popover_header', 'popover_shape', 'popover_values', 'popover_view', 'revoke_tool_approval', 'save_settings', 'service_state', 'set_app_key', 'set_autostart', 'set_update_check', 'settings', 'settings_*', 'start_check_updates', 'start_install_service', 'start_repair_service', 'start_uninstall_service', 'update_check_status', 'update_notices'],
  },
  'pw.create-app': {
    id: 'pw.create-app',
    kind: 'form',
    modality: 'modal',
    width: 400,
    widthStep: 'narrow',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: [],
  },
  'pw.mcp-key': {
    id: 'pw.mcp-key',
    kind: 'form',
    modality: 'modal',
    width: 400,
    widthStep: 'narrow',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: [],
  },
  'pw.key-secret': {
    id: 'pw.key-secret',
    kind: 'decision',
    modality: 'modal',
    width: 400,
    widthStep: 'narrow',
    height: null,
    instanced: false,
    requiredChoice: true,
    commands: [],
  },
  'pw.quit': {
    id: 'pw.quit',
    kind: 'decision',
    modality: 'modal',
    width: 560,
    widthStep: 'medium',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: [],
  },
  'pw.service-consent': {
    id: 'pw.service-consent',
    kind: 'decision',
    modality: 'modal',
    width: 560,
    widthStep: 'medium',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: [],
  },
} as const;

export type DialogId = keyof typeof DIALOG_CONTRACTS;
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];

/** The commands every dialog window of this owner's app may call besides the shell's base set. */
export const DIALOG_BASE = ['window_pong', 'window_ready'] as const;
