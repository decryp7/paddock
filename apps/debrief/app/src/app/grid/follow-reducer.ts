/**
 * The follow and hold rules of the live-tail grid as one pure table (ADR 0072, decision 3). `holding` mirrors the user's choice
 * (Follow off, or a pause by a zoom or pan: new events are counted, not shown); `pinned` says the view is on the newest end (the
 * last row oldest first, the first row newest first), stored and not re-derived from the position at each round.
 */
export interface FollowState {
  holding: boolean;
  pinned: boolean;
  /** The sequence stamp (`nextRequestSeq`) of the last user take-over; 0 when there was none. */
  takeOver: number;
}

export type FollowEvent =
  /** New live data under the same query (a refresh round would start). */
  | { type: 'data' }
  /** The viewport changed size. */
  | { type: 'resize'; seq: number }
  /** An input event that moves the view away from the newest end (wheel, scrollbar press, touch, a key away). */
  | { type: 'userTookOver'; seq: number }
  /**
   * The view is at a new position. `byUser`: the user moved it (a scroll event that was not the grid's own write, a key, a jump):
   * `pinned` becomes whether it is on the newest end, and the round lets go. Otherwise the grid moved it itself (a live write, a resize):
   * landing on the newest end pins, anything else leaves `pinned` alone.
   */
  | { type: 'positioned'; atNewest: boolean; byUser: boolean }
  /** A page outside a live round reported a larger total (the end moved). */
  | { type: 'grew' }
  /** Holding ended without Follow on or Resume (live tail came back, a pause was cleared). */
  | { type: 'released' }
  /** Follow off, or a manual zoom or pan. */
  | { type: 'hold' }
  /** The pill: show the newest events; still holding. */
  | { type: 'reveal'; seq: number }
  /** Follow on or Resume. */
  | { type: 'followOn'; seq: number }
  /** A new query or sort order: the view goes to the top. */
  | { type: 'reset'; seq: number };

/** What the grid must do for the event. */
export interface FollowAction {
  /** The refresh round's mode (see `LiveMode`); `none` starts no round. */
  round: 'end' | 'shift' | 'keep' | 'none';
  /** Scroll to the newest end of the rows there are (after the render: the size may change). */
  goNewest: boolean;
  /** Write the new newest end after the render (a resize while pinned). */
  writeNewest: boolean;
  /** Keep the top row where it is (a resize while not pinned). */
  keepTopRow: boolean;
  /** Count the new events instead of showing them. */
  count: boolean;
  /** The round's hold on the view ends now, and a pending scroll write is dropped. */
  endHold: boolean;
  /** What a total that grew outside a round does to the view: scroll to the new end, shift by the extra rows, or nothing. */
  growth: 'end' | 'shift' | 'none';
}

const NONE: FollowAction = { round: 'none', goNewest: false, writeNewest: false, keepTopRow: false, count: false, endHold: false, growth: 'none' };

/** The next state and the action for `event`; `descending` is the sort order (newest first puts the newest end at the top). */
export function followStep(state: FollowState, event: FollowEvent, descending: boolean): { state: FollowState; action: FollowAction } {
  const newest: FollowAction['round'] = descending ? 'keep' : 'end';
  // One rule for every request that moves the view: stamped before the last take-over, it is stale (an effect applied it late) and the
  // user's move wins. A stale reveal or Follow on still means "reload the rows": a round that keeps the view where it is (as for a following view that is not pinned).
  if ((event.type === 'reveal' || event.type === 'followOn' || event.type === 'resize' || event.type === 'reset') && event.seq < state.takeOver) {
    return { state, action: event.type === 'reveal' || event.type === 'followOn' ? { ...NONE, round: descending ? 'shift' : 'keep' } : NONE };
  }
  switch (event.type) {
    case 'data':
      if (state.holding) return { state, action: { ...NONE, count: true } };
      if (state.pinned) return { state, action: { ...NONE, round: newest } };
      return { state, action: { ...NONE, round: descending ? 'shift' : 'keep' } };
    case 'resize':
      return { state, action: state.pinned ? { ...NONE, writeNewest: true } : { ...NONE, keepTopRow: true } };
    case 'userTookOver':
      return { state: { ...state, pinned: false, takeOver: event.seq }, action: { ...NONE, endHold: true } };
    case 'positioned':
      if (event.byUser) return { state: { ...state, pinned: event.atNewest }, action: { ...NONE, endHold: true } };
      return { state: event.atNewest ? { ...state, pinned: true } : state, action: NONE };
    case 'grew': {
      // Pinned: the end moved, so the view follows (oldest first; newest first the newest rows are at the top and stay there). A held
      // view that is not pinned keeps its rows. A following view that is not pinned oldest first has rows added below it: nothing;
      // newest first they are inserted above it: shift.
      if (state.pinned) return { state, action: { ...NONE, growth: descending ? 'none' : 'end' } };
      if (state.holding) return { state, action: NONE };
      return { state, action: { ...NONE, growth: descending ? 'shift' : 'none' } };
    }
    case 'released':
      return { state: { ...state, holding: false }, action: NONE };
    case 'hold':
      return { state: { ...state, holding: true }, action: NONE };
    case 'reveal':
      return { state: { ...state, pinned: true }, action: { ...NONE, round: newest, goNewest: true } };
    case 'followOn':
      return { state: { ...state, holding: false, pinned: true }, action: { ...NONE, round: newest, goNewest: true } };
    case 'reset':
      return { state: { ...state, pinned: descending }, action: NONE };
  }
}
