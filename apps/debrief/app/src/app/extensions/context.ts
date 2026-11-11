import type { Signal } from '@angular/core';
import type { ShellContext } from '@decryp7/paddock-ui/shell';
import type { FieldFilter, Query, RecordView, WorkspaceSummary } from '../core/api.types';

/** An event's id in the store (`RecordView.id`). Ids stay below 2^53 (debrief-1-2-api.md §2.2). */
export type EventId = number;

/** A closed time interval in epoch ms. */
export interface TimeRange {
  readonly from: number;
  readonly to: number;
}

/**
 * Which window's workspace a command acts on: the window's id, never the store itself, so a command handed the context
 * cannot reach into another window's state (debrief-1-2-api.md §14.3).
 */
export interface WorkspaceHandle {
  /** `main`, or the pop-out's panel window label. */
  readonly window: string;
}

/** The zone times are shown in (§10.4). `logs` (each source's own zone) comes with the time model in 1.2's 2.x rows. */
export type TimeDisplay = 'utc' | 'local' | 'logs';

/**
 * Debrief's part of the shell context (`ShellContext<DebriefContext>`): what a contribution's live functions (`when`,
 * `run`, `badge`, ...) may read and do. It replaces the old `CommandContext` interface: the four read-only signals of
 * debrief-1-2-api.md §14.3 and the narrow actions plugins already had. Plugins never touch the stores; they observe
 * through `DebriefEvents` and act through this context.
 */
export interface DebriefContext {
  /** The window's workspace (its id, not the store). */
  readonly workspace: WorkspaceHandle;
  /**
   * The selected events: the grid's multi-selection, else the one selected event, else none. M1 keeps today's ids;
   * it becomes `Signal<KeySelection<EventId>>` in 3.7.
   */
  readonly selection: Signal<readonly EventId[]>;
  /** The query's time range, or null when the query has none. */
  readonly timeRange: Signal<TimeRange | null>;
  /** The zone times are shown in. */
  readonly timeDisplay: Signal<TimeDisplay>;
  /** The current workspace summary (null before the first load). */
  readonly summary: WorkspaceSummary | null;
  /** Opens files or folders in the workspace. */
  openPaths(paths: string[]): Promise<void>;
  /** Applies a partial query (search text, levels, range, ...). */
  setQuery(patch: Partial<Query>): void;
  /** Adds a field filter pill. */
  addFilter(filter: FieldFilter): void;
  /** Selects a record (shown in the inspector). */
  selectRecord(record: RecordView | null): void;
  /**
   * Reveals a record by id: loads the grid page containing it, scrolls to the row and selects it. Resolves once the grid
   * has done so or reported that the record is not in the current view.
   */
  jumpTo(id: EventId): Promise<void>;
  /** Opens (if needed) and focuses a view by id. */
  showView(id: string): void;
  /**
   * Offers the window arrangement for what the user just opened (`profiling` after a performance capture): switches to
   * that perspective once per window, with an Undo in the notice. Unknown ids and repeats do nothing.
   */
  suggestPerspective?(id: string): void;
  /** Reports a problem to the user. */
  notifyError(message: string): void;
}

/** What Debrief's commands, activity items and other contributions receive: the shell's part and Debrief's. */
export type CommandContext = ShellContext<DebriefContext>;
