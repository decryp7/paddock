/**
 * `@decryp7/paddock-ui/shell/undo`: undo and the activity log of a window (plan X44, README principle "changes have an
 * undo"). Two halves over `paddock_jobs::journal`:
 *
 * - `UndoStore`: the history of the user's own edits (the port of Debrief's `UndoStack`): `record` / `execute` closures with
 *   labels, merge keys, groups, undo and redo, Edit > Undo and Redo (`UNDO_CONTRIBUTIONS`, Mod+Z) and the "Done, Undo"
 *   toast (`pd-undo-toast`).
 * - `ActivityStore` and `pd-activity-page`: the journal of what the app did on the user's behalf, with Undo while the app
 *   says it is still possible. The app owns the commands (`activity_list`, `activity_undo`) because undoing is its work;
 *   `TauriActivityBackend` calls them and listens on `paddock://activity`.
 */
export { UNDO_CLOCK, UNDO_LIMIT, MERGE_WINDOW_MS, UNDO_TOAST_MS, type UndoClock } from './src/undo-config';
export { UndoStore, UNDO_NOTICE_KINDS, type UndoStoreType, type UndoableAction } from './src/undo.store';
export { PdUndoToast } from './src/undo-toast';
export { provideShellUndo } from './src/provide-undo';
export { UNDO_CONTRIBUTIONS, UNDO_COMMAND, REDO_COMMAND, stepHistory, type UndoContext, type UndoControls } from './src/undo.contributions';
export { ACTIVITY_EVENT, parseActivityEntries, parseActivityEntry, type ActivityEntry, type ActivityUndoState } from './src/activity-wire';
export {
  ACTIVITY_PAGE_SIZE,
  ActivityBackend,
  DEFAULT_ACTIVITY_COMMANDS,
  MemoryActivityBackend,
  TauriActivityBackend,
  type ActivityCommands,
  type ActivityIpc,
  type Unlisten,
} from './src/activity-backend';
export { ActivityStore, ACTIVITY_NOTICE_KIND, mergeEntry, type ActivityStoreType } from './src/activity.store';
export { provideShellActivity, type ShellActivityOptions } from './src/provide-activity';
// The page component loads on first visit (a chunk of its own): its class is exported as a type only (plan S16).
export type { PdActivityPage } from './src/activity-page';
export { ACTIVITY_CONTRIBUTIONS, ACTIVITY_PAGE_ID } from './src/activity.contributions';
