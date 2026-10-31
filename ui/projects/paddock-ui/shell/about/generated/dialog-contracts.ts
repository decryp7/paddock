// Generated from the dialog table of paddock-updater (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-updater --lib generated.

/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], { title, result, load })`. */
export const DIALOG_CONTRACTS = {
  'pd.about': {
    id: 'pd.about',
    kind: 'viewer',
    modality: 'modeless',
    width: 720,
    widthStep: 'wide',
    height: 560,
    instanced: false,
    requiredChoice: false,
    commands: ['about_info', 'about_legal', 'files_open_url'],
  },
  'pd.whats-new': {
    id: 'pd.whats-new',
    kind: 'viewer',
    modality: 'modeless',
    width: 720,
    widthStep: 'wide',
    height: 560,
    instanced: false,
    requiredChoice: false,
    commands: ['about_whats_new'],
  },
  'pd.updates': {
    id: 'pd.updates',
    kind: 'form',
    modality: 'modeless',
    width: 560,
    widthStep: 'medium',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: ['files_open_url', 'updates_cancel', 'updates_check', 'updates_download', 'updates_install_on_quit', 'updates_restart', 'updates_set_preferences', 'updates_status'],
  },
} as const;

export type DialogId = keyof typeof DIALOG_CONTRACTS;
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];

/** The commands every dialog window of this owner's app may call besides the shell's base set. */
export const DIALOG_BASE = [] as const;
