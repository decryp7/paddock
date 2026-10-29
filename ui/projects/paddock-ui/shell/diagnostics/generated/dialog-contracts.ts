// Generated from the dialog table of paddock-diag (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-diag --all-features --lib generated.

/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], { title, result, load })`. */
export const DIALOG_CONTRACTS = {
  'pd.report-problem': {
    id: 'pd.report-problem',
    kind: 'form',
    modality: 'modeless',
    width: 720,
    widthStep: 'wide',
    height: null,
    instanced: false,
    requiredChoice: false,
    commands: ['diagnostics_cancel', 'diagnostics_preview', 'diagnostics_save', 'log_from_webview'],
  },
} as const;

export type DialogId = keyof typeof DIALOG_CONTRACTS;
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];

/** The commands every dialog window of this owner's app may call besides the shell's base set. */
export const DIALOG_BASE = [] as const;
