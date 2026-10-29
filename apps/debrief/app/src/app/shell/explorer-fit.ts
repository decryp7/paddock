import { EXPLORER_WIDTH } from '../core/workspace/ui-prefs.store';

/** What the layout plan says about the Explorer for a window: the plan's rooms and verdict, and the user's wish. */
export interface ExplorerFitInput {
  /** The Explorer is open. */
  readonly open: boolean;
  /** The user set the width (`UiPrefsStore.explorerWidthChosen`): it wins over the other columns. */
  readonly chosen: boolean;
  /** The width the user asked for, px. */
  readonly wish: number;
  /** The widest the Explorer may be while the Events view keeps its minimum (the sash cap), px. */
  readonly hardRoom: number;
  /** The widest it may be while every other column keeps its own, px (what the default width yields to). */
  readonly columnRoom: number;
}

/** What to show and what to tell the dock. */
export interface ExplorerFit {
  /** The width to show, px. */
  readonly shown: number;
  /** What the Explorer can give up beside the dock, px: the dock plans as if it had it (0 for a chosen width and when closed). */
  readonly yield: number;
}

/** The width the Explorer is shown at and the room it lends the dock. The user's width, within the room it has. */
export function fitExplorer(i: ExplorerFitInput): ExplorerFit {
  const shown = Math.min(i.wish, i.chosen ? i.hardRoom : i.columnRoom);
  return { shown, yield: i.open && !i.chosen ? Math.max(0, shown - EXPLORER_WIDTH.min) : 0 };
}

/** What the follower does with the sidebar after a plan. */
export type ExplorerAction = 'close' | 'open' | 'none';

/**
 * Closes an open Explorer the plan has no room for (also one the user has just opened by hand: there is no overlay mode) and
 * reopens one this follower closed when there is room again; an Explorer the user closed is never reopened.
 * `closedByNarrowing` is whether the follower closed the sidebar last.
 */
export function explorerAction(open: boolean, verdict: 'hide' | 'show' | 'keep', closedByNarrowing: boolean): { action: ExplorerAction; closedByNarrowing: boolean } {
  if (open) return verdict === 'hide' ? { action: 'close', closedByNarrowing: true } : { action: 'none', closedByNarrowing: false };
  return verdict === 'show' && closedByNarrowing ? { action: 'open', closedByNarrowing: false } : { action: 'none', closedByNarrowing };
}
