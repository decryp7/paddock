// Generated from the dialog table of paddock-shell (set-up) (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --lib setup::generated.

/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], { title, result, load })`. */
export const DIALOG_CONTRACTS = {
  'pd.setup': {
    id: 'pd.setup',
    kind: 'form',
    modality: 'modeless',
    width: 560,
    widthStep: 'medium',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: ['setup_act', 'setup_finish', 'setup_repair', 'setup_skip', 'setup_state'],
  },
} as const;

export type DialogId = keyof typeof DIALOG_CONTRACTS;
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];

/** The commands every dialog window of this owner's app may call besides the shell's base set. */
export const DIALOG_BASE = [] as const;
