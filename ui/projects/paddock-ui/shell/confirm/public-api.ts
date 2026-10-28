/**
 * `@decryp7/paddock-ui/shell/confirm`: `pd.confirm`, the one shared confirmation (ADR 0026 §5): its contract (Rust's
 * `paddock_shell::DIALOGS`), `defineConfirmDialog` for the app's registry and `confirm()` for an opener. The component is
 * in `@decryp7/paddock-ui/shell/confirm/dialog`, a chunk only its window loads.
 */
export { CONFIRM_DIALOG_ID, confirm, defineConfirmDialog, type ConfirmDialogOptions, type ConfirmInput, type ConfirmOpener } from './src/confirm';
export { DIALOG_CONTRACTS as CONFIRM_DIALOG_CONTRACTS } from './generated/dialog-contracts';
