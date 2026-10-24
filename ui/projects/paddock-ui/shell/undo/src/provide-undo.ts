import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { UndoStore } from './undo.store';

/**
 * The undo history of one window (rules 32, 34): the store only. Each window that records edits calls it in its own
 * bootstrap; the history is the window's own and is cleared when the work it refers to is replaced (`clear`). Add
 * `undo: inject(UndoStore)` to the app's context and register `UNDO_CONTRIBUTIONS` for Edit > Undo and Redo.
 */
export function provideShellUndo(): EnvironmentProviders {
  return makeEnvironmentProviders([UndoStore]);
}
