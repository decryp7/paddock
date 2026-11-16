import { FollowEvent, FollowState, followStep } from './follow-reducer';

// ADR 0072, decision 3: the follow and hold rules as one pure table.
describe('followStep', () => {
  const state = (holding: boolean, pinned: boolean): FollowState => ({ holding, pinned });

  type Row = [string, FollowState, FollowEvent, boolean, Partial<ReturnType<typeof followStep>['action']> & { state?: FollowState }];
  const table: Row[] = [
    // Following, pinned
    ['following pinned, new data, oldest first: round in end mode', state(false, true), { type: 'data' }, false, { round: 'end', state: state(false, true) }],
    ['following pinned, new data, newest first: the view stays at the top', state(false, true), { type: 'data' }, true, { round: 'keep', state: state(false, true) }],
    ['following pinned, resize: write the new end after the render', state(false, true), { type: 'resize' }, false, { round: 'none', writeNewest: true, state: state(false, true) }],
    ['following pinned, user took over: unpinned, the round lets go', state(false, true), { type: 'userTookOver' }, false, { endHold: true, state: state(false, false) }],
    // Following, not pinned
    ['following not pinned, new data, oldest first: keep', state(false, false), { type: 'data' }, false, { round: 'keep', state: state(false, false) }],
    ['following not pinned, new data, newest first: shift', state(false, false), { type: 'data' }, true, { round: 'shift', state: state(false, false) }],
    ['following not pinned, resize: keep the top row', state(false, false), { type: 'resize' }, false, { keepTopRow: true, writeNewest: false, state: state(false, false) }],
    ['following not pinned, the user scrolls to the newest end: pinned', state(false, false), { type: 'userAtNewest' }, false, { state: state(false, true) }],
    // Holding
    ['following, Follow off or a pause: holding, no round', state(false, true), { type: 'hold' }, false, { round: 'none', state: state(true, true) }],
    ['holding, new data: count only', state(true, true), { type: 'data' }, false, { round: 'none', count: true, state: state(true, true) }],
    ['holding, new data, not pinned: count only', state(true, false), { type: 'data' }, true, { round: 'none', count: true, state: state(true, false) }],
    ['holding, reveal, oldest first: round in end mode, pinned, still holding', state(true, false), { type: 'reveal' }, false, { round: 'end', goNewest: true, state: state(true, true) }],
    ['holding, reveal, newest first: back to the top, pinned, still holding', state(true, false), { type: 'reveal' }, true, { round: 'keep', goNewest: true, state: state(true, true) }],
    ['holding, resize while pinned (a reveal): write the new end', state(true, true), { type: 'resize' }, false, { writeNewest: true, state: state(true, true) }],
    ['holding, resize while not pinned: keep the top row', state(true, false), { type: 'resize' }, false, { keepTopRow: true, state: state(true, false) }],
    ['holding, user took over after a reveal: unpinned, still holding', state(true, true), { type: 'userTookOver' }, false, { endHold: true, state: state(true, false) }],
    ['holding, Follow on or Resume: following, pinned, round in end mode', state(true, false), { type: 'follow' }, false, { round: 'end', goNewest: true, state: state(false, true) }],
    ['holding, Follow on, newest first: following, pinned, back to the top', state(true, false), { type: 'follow' }, true, { round: 'keep', goNewest: true, state: state(false, true) }],
    // Any state
    ['a new query or sort order, oldest first: the top is not the newest end', state(false, true), { type: 'reset' }, false, { round: 'none', state: state(false, false) }],
    ['a new query or sort order, newest first: the top is the newest end', state(true, false), { type: 'reset' }, true, { round: 'none', state: state(true, true) }],
  ];

  it.each(table)('%s', (_name, from, event, descending, expected) => {
    const { state: next, action } = followStep(from, event, descending);
    const { state: nextState, ...actionFields } = expected;
    if (nextState) expect(next).toEqual(nextState);
    expect(action).toMatchObject(actionFields);
  });

  it('only a reveal, Follow on and a user take-over change `pinned` besides the position events, and nothing else touches `holding`', () => {
    for (const event of ['data', 'resize'] as const) {
      for (const holding of [true, false]) {
        for (const pinned of [true, false]) {
          for (const descending of [true, false]) expect(followStep(state(holding, pinned), { type: event }, descending).state).toEqual(state(holding, pinned));
        }
      }
    }
  });
});
