/**
 * The follow and hold rules of the live-tail grid as one pure table (ADR 0072, decision 3). `holding` mirrors the user's choice
 * (Follow off, or a pause by a zoom or pan: new events are counted, not shown); `pinned` says the view is on the newest end (the
 * last row oldest first, the first row newest first), stored and not re-derived from the position at each round.
 */
export interface FollowState {
  holding: boolean;
  pinned: boolean;
}

export type FollowEvent =
  /** New live data under the same query (a refresh round would start). */
  | { type: 'data' }
  /** The viewport changed size. */
  | { type: 'resize' }
  /** An input event (wheel, scrollbar, touch, a moving key) or a scroll that was not the grid's own. */
  | { type: 'userTookOver' }
  /** The user scrolled back to the newest end. */
  | { type: 'userAtNewest' }
  /** Follow off, or a manual zoom or pan. */
  | { type: 'hold' }
  /** The pill: show the newest events; still holding. */
  | { type: 'reveal' }
  /** Follow on or Resume. */
  | { type: 'follow' }
  /** A new query or sort order: the view goes to the top. */
  | { type: 'reset' };

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
}

const NONE: FollowAction = { round: 'none', goNewest: false, writeNewest: false, keepTopRow: false, count: false, endHold: false };

/** The next state and the action for `event`; `descending` is the sort order (newest first puts the newest end at the top). */
export function followStep(state: FollowState, event: FollowEvent, descending: boolean): { state: FollowState; action: FollowAction } {
  const newest: FollowAction['round'] = descending ? 'keep' : 'end';
  switch (event.type) {
    case 'data':
      if (state.holding) return { state, action: { ...NONE, count: true } };
      if (state.pinned) return { state, action: { ...NONE, round: newest } };
      return { state, action: { ...NONE, round: descending ? 'shift' : 'keep' } };
    case 'resize':
      return { state, action: state.pinned ? { ...NONE, writeNewest: true } : { ...NONE, keepTopRow: true } };
    case 'userTookOver':
      return { state: { ...state, pinned: false }, action: { ...NONE, endHold: true } };
    case 'userAtNewest':
      return { state: { ...state, pinned: true }, action: NONE };
    case 'hold':
      return { state: { ...state, holding: true }, action: NONE };
    case 'reveal':
      return { state: { ...state, pinned: true }, action: { ...NONE, round: newest, goNewest: true } };
    case 'follow':
      return { state: { holding: false, pinned: true }, action: { ...NONE, round: newest, goNewest: true } };
    case 'reset':
      return { state: { ...state, pinned: descending }, action: NONE };
  }
}
