import type { Signal } from '@angular/core';
import { defineCommand, defineKeybinding, defineMenuItem, defineNotificationKind, msg, type ContributionBatch, type ShellContext } from '@decryp7/paddock-ui/shell';
import { UNDO_NOTICE_KINDS } from './undo-kinds';

/** What the undo commands need from the app's context: the window's `UndoStore` (any object with these members). */
export interface UndoControls {
  readonly canUndo: Signal<boolean>;
  readonly canRedo: Signal<boolean>;
  readonly undoLabel: Signal<string | null>;
  readonly redoLabel: Signal<string | null>;
  undo(): Promise<boolean>;
  redo(): Promise<boolean>;
}

/** The part of the app's context the undo commands use: add `undo: inject(UndoStore)` to it. */
export interface UndoContext {
  readonly undo: UndoControls;
}

export const UNDO_COMMAND = 'pd.edit.undo';
export const REDO_COMMAND = 'pd.edit.redo';

/** Undoes or redoes the latest edit and says what happened (or that there was nothing to do). */
export async function stepHistory(ctx: ShellContext<UndoContext>, op: 'undo' | 'redo'): Promise<void> {
  const label = op === 'undo' ? ctx.undo.undoLabel() : ctx.undo.redoLabel();
  if (!label) {
    ctx.notify({ level: 'info', message: op === 'undo' ? $localize`:@@pd.undo.nothingToUndo:Nothing to undo` : $localize`:@@pd.undo.nothingToRedo:Nothing to redo` });
    return;
  }
  if (await (op === 'undo' ? ctx.undo.undo() : ctx.undo.redo())) {
    ctx.notify({ level: 'info', message: op === 'undo' ? $localize`:@@pd.undo.undone:Undone: ${label}:label:` : $localize`:@@pd.undo.redone:Redone: ${label}:label:` });
  }
}

/**
 * Edit > Undo and Redo (Mod+Z, Mod+Shift+Z) over the window's `UndoStore`. The app's context must carry `undo` (see
 * `UndoContext`). Static data only (plan §9): the store comes from `provideShellUndo` in the window's bootstrap.
 */
export const UNDO_CONTRIBUTIONS: ContributionBatch<UndoContext> = {
  commands: [
    defineCommand<UndoContext>({
      id: UNDO_COMMAND,
      title: msg('pd.undo.cmd.undo', 'Undo'),
      category: msg('pd.undo.cmd.category', 'Edit'),
      enabled: (ctx) => ctx.undo.canUndo(),
      run: (ctx) => stepHistory(ctx, 'undo'),
    }),
    defineCommand<UndoContext>({
      id: REDO_COMMAND,
      title: msg('pd.undo.cmd.redo', 'Redo'),
      category: msg('pd.undo.cmd.category', 'Edit'),
      enabled: (ctx) => ctx.undo.canRedo(),
      run: (ctx) => stepHistory(ctx, 'redo'),
    }),
  ],
  keybindings: [defineKeybinding({ command: UNDO_COMMAND, key: 'Mod+Z' }), defineKeybinding({ command: REDO_COMMAND, key: 'Mod+Shift+Z' })],
  menus: [
    defineMenuItem<UndoContext>({ menu: 'edit', command: UNDO_COMMAND, group: 'history', order: 100 }),
    defineMenuItem<UndoContext>({ menu: 'edit', command: REDO_COMMAND, group: 'history', order: 200 }),
  ],
  notificationKinds: [defineNotificationKind({ id: UNDO_NOTICE_KINDS.failed, title: msg('pd.undo.notification.failed', 'Could not undo'), actions: [] })],
};
