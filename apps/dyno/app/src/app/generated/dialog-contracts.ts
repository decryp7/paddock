// Generated from the dialog table of the app (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell.

/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], { title, result, load })`. */
export const DIALOG_CONTRACTS = {
  'dy.alert-rule': {
    id: 'dy.alert-rule',
    kind: 'form',
    modality: 'modal',
    width: 560,
    widthStep: 'medium',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: ['dyno'],
  },
  'dy.file-hash': {
    id: 'dy.file-hash',
    kind: 'viewer',
    modality: 'modeless',
    width: 720,
    widthStep: 'wide',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: ['dyno'],
  },
  'dy.lockers': {
    id: 'dy.lockers',
    kind: 'tool',
    modality: 'modeless',
    width: 560,
    widthStep: null,
    height: 480,
    instanced: false,
    requiredChoice: false,
    commands: ['dyno'],
  },
  'dy.web-search': {
    id: 'dy.web-search',
    kind: 'form',
    modality: 'modal',
    width: 400,
    widthStep: 'narrow',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: ['dyno'],
  },
  'pd.settings': {
    id: 'pd.settings',
    kind: 'tool',
    modality: 'modeless',
    width: 880,
    widthStep: null,
    height: 640,
    instanced: false,
    requiredChoice: false,
    commands: ['dyno', 'settings_*'],
  },
} as const;

export type DialogId = keyof typeof DIALOG_CONTRACTS;
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];

/** The commands every dialog window of this owner's app may call besides the shell's base set. */
export const DIALOG_BASE = ['window_pong', 'window_ready'] as const;
