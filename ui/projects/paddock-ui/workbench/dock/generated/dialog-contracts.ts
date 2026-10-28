// Generated from the dialog table of paddock-workbench (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-workbench --all-features --lib generated.

/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], { title, result, load })`. */
export const DIALOG_CONTRACTS = {
  'pd.wb.dock.layout-save': {
    id: 'pd.wb.dock.layout-save',
    kind: 'form',
    modality: 'modal',
    width: 400,
    widthStep: 'narrow',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: [],
  },
  'pd.wb.dock.layout-manage': {
    id: 'pd.wb.dock.layout-manage',
    kind: 'tool',
    modality: 'modeless',
    width: 480,
    widthStep: null,
    height: 360,
    instanced: false,
    requiredChoice: false,
    commands: ['layout_get', 'layout_set'],
  },
} as const;

export type DialogId = keyof typeof DIALOG_CONTRACTS;
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];

/** The commands every dialog window of this owner's app may call besides the shell's base set. */
export const DIALOG_BASE = [] as const;
