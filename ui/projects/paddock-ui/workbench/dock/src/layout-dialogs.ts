import { defineDialog, msg, type, type DialogDef } from '@decryp7/paddock-ui/shell';
import { DOCK_OPTIONS, type DockOptions } from './dock-options';
import { DOCK_LAYOUT_STORE, InMemoryDockLayoutStore } from './dock-store';
import type { LayoutManageResult } from './layout-manage-dialog';
import { DIALOG_CONTRACTS } from '../generated/dialog-contracts';

/** Id of the save dialog (stable: its window route, remembered bounds, test ids). */
export const LAYOUT_SAVE_DIALOG_ID = 'pd.wb.dock.layout-save';
/** Id of the manage dialog. */
export const LAYOUT_MANAGE_DIALOG_ID = 'pd.wb.dock.layout-manage';

/**
 * The dock's two layout dialogs as real windows (rule 17, ADR 0009). `options.store` is where the layouts live: the manage
 * dialog reads and edits the layout document through it, so a dialog window needs the same kind of store as the main window
 * (an `IpcDockLayoutStore`) and the same `storageKey`; the window boots with only these providers (rule 34).
 *
 * - Save layout: a modal `form` whose input is `{ names, suggested }`; resolves the new name (cancelled: nothing). Calls nothing.
 * - Manage layouts: a modeless `tool`; resolves `{ apply }` when Apply was pressed. Reads and writes the layouts document of
 *   the store (the engine's `paddock-workbench` plugin commands `layout_get` and `layout_set`, scoped by that plugin's
 *   capability, not by the dialog guard); no other command.
 */
export function dockLayoutDialogs(options: DockOptions = {}): readonly [DialogDef<typeof LAYOUT_SAVE_DIALOG_ID, string, string>, DialogDef<typeof LAYOUT_MANAGE_DIALOG_ID, LayoutManageResult, string>] {
  const providers = () =>
    Promise.resolve([
      { provide: DOCK_OPTIONS, useValue: options },
      { provide: DOCK_LAYOUT_STORE, useValue: options.store ?? new InMemoryDockLayoutStore() },
    ]);
  const save = defineDialog(DIALOG_CONTRACTS[LAYOUT_SAVE_DIALOG_ID], {
    title: msg('pd.wb.dock.layout.save-title', 'Save layout'),
    result: type<string>(),
    providers,
    load: () => import('./layout-save-dialog').then((m) => m.PdLayoutSaveDialog),
  });
  const manage = defineDialog(DIALOG_CONTRACTS[LAYOUT_MANAGE_DIALOG_ID], {
    title: msg('pd.wb.dock.layout.manage-title', 'Manage layouts'),
    result: type<LayoutManageResult>(),
    providers,
    load: () => import('./layout-manage-dialog').then((m) => m.PdLayoutManageDialog),
  });
  return [save, manage];
}
