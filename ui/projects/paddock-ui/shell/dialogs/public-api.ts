/**
 * `@decryp7/paddock-ui/shell/dialogs`: dialogs as OS windows (ADR 0009, rule 17; plan S6). Dialogs are declared with
 * `defineDialog` (`@decryp7/paddock-ui/shell`); this entry opens them (`DialogService`, typed by the definition), shows
 * them as windows (`WindowDialogBackend` over `paddock-dialogs`) or in the page (`InPageDialogBackend`, `pd-dialog-host`),
 * boots each dialog window on its own (`dialogWindowConfig`, `DialogShell`), keeps the kind policy (size, fit, keys,
 * a visible Close/Cancel), the modal gate and remembered bounds. The contracts are Rust's (ADR 0026 §3).
 */
export {
  DIALOG_MIN,
  VIEWER_MIN,
  TOOL_DEFAULT,
  dismissOf,
  modalityOf,
  resolveSize,
  fitsContent,
  growsOnly,
  raiseToKindMinimum,
  instanceKey,
  type DialogDismiss,
  type ResolvedSize,
} from './src/kind';
export { commandCovered, allowedCommands } from './src/contracts';
export { DIALOG_IO, BROWSER_DIALOG_RESULT_KEY, browserDialogIo, tauriDialogIo, type DialogIo, type DialogMeasure, type TauriBridge, type Unlisten } from './src/io';
export { DIALOG_ROUTE, DIALOG_ROUTE_PATH, parseDialogRoute, dialogHash, type DialogRoute } from './src/route';
export { dialogKeyAction, findDefaultAction, hasCloseAction, type DialogKeyAction, type DialogKeyEvent } from './src/keys';
export {
  DIALOG_BOUNDS_KEY,
  DIALOG_BOUNDS_VERSION,
  MAX_REMEMBERED,
  isRememberedPlace,
  loadDialogBounds,
  rememberDialogBounds,
  type BoundsStorage,
} from './src/bounds';
export { ModalGate } from './src/modal-gate';
export { DeniedCommands, isDenial } from './src/denied';
export {
  DIALOG_REF,
  DIALOG_INPUT,
  DIALOG_LOOKUP,
  DialogPresenter,
  type DialogRef,
  type DialogContent,
  type DialogResultOf,
  type DialogInputsOf,
  type DialogInputsOfDef,
  type DialogLookup,
  type DialogShowRequest,
  type DialogSession,
} from './src/types';
export { InPageDialogBackend, raise, type InPageDialog } from './src/in-page';
export { WindowDialogBackend, windowRequest, parseResult, parseClosed } from './src/window-backend';
export { DialogService, type DialogOpenOptions, type AnyDialogDef } from './src/dialog.service';
export { DialogHost, InPageDialogView } from './src/dialog-host';
export { DialogShell } from './src/dialog-shell';
export {
  provideDialogs,
  openDialogPort,
  dialogWindowConfig,
  mountDialogWindowHost,
  type DialogsOptions,
  type DialogWindowOptions,
} from './src/bootstrap';
