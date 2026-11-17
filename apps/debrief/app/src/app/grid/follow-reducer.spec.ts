import { FollowEvent, FollowState, followStep } from './follow-reducer';

// ADR 0072, decision 3: the follow and hold rules as one pure table.
describe('followStep', () => {
  const state = (holding: boolean, pinned: boolean, takeOver = 0): FollowState => ({ holding, pinned, takeOver });

  type Row = [string, FollowState, FollowEvent, boolean, Partial<ReturnType<typeof followStep>['action']> & { state?: FollowState }];
  const table: Row[] = [
    // Following, pinned
    ['following pinned, new data, oldest first: round in end mode', state(false, true), { type: 'data' }, false, { round: 'end', state: state(false, true) }],
    ['following pinned, new data, newest first: the view stays at the top', state(false, true), { type: 'data' }, true, { round: 'keep', state: state(false, true) }],
    ['following pinned, resize: write the new end after the render', state(false, true), { type: 'resize', seq: 5 }, false, { round: 'none', writeNewest: true, state: state(false, true) }],
    ['following pinned, user took over: unpinned, the round lets go', state(false, true), { type: 'userTookOver', seq: 5 }, false, { endHold: true, state: state(false, false, 5) }],
    // Following, not pinned
    ['following not pinned, new data, oldest first: keep', state(false, false), { type: 'data' }, false, { round: 'keep', state: state(false, false) }],
    ['following not pinned, new data, newest first: shift', state(false, false), { type: 'data' }, true, { round: 'shift', state: state(false, false) }],
    ['following not pinned, resize: keep the top row', state(false, false), { type: 'resize', seq: 5 }, false, { keepTopRow: true, writeNewest: false, state: state(false, false) }],
    // Holding
    ['following, Follow off or a pause: holding, no round', state(false, true), { type: 'hold' }, false, { round: 'none', state: state(true, true) }],
    ['holding, new data: count only', state(true, true), { type: 'data' }, false, { round: 'none', count: true, state: state(true, true) }],
    ['holding, new data, not pinned: count only', state(true, false), { type: 'data' }, true, { round: 'none', count: true, state: state(true, false) }],
    ['holding, reveal, oldest first: round in end mode, pinned, still holding', state(true, false), { type: 'reveal', seq: 5 }, false, { round: 'end', goNewest: true, state: state(true, true) }],
    ['holding, reveal, newest first: back to the top, pinned, still holding', state(true, false), { type: 'reveal', seq: 5 }, true, { round: 'keep', goNewest: true, state: state(true, true) }],
    ['holding, resize while pinned (a reveal): write the new end', state(true, true), { type: 'resize', seq: 5 }, false, { writeNewest: true, state: state(true, true) }],
    ['holding, resize while not pinned: keep the top row', state(true, false), { type: 'resize', seq: 5 }, false, { keepTopRow: true, state: state(true, false) }],
    ['holding, user took over after a reveal: unpinned, still holding', state(true, true), { type: 'userTookOver', seq: 5 }, false, { endHold: true, state: state(true, false, 5) }],
    ['holding, Follow on or Resume: following, pinned, round in end mode', state(true, false), { type: 'followOn', seq: 5 }, false, { round: 'end', goNewest: true, state: state(false, true) }],
    ['holding, Follow on, newest first: following, pinned, back to the top', state(true, false), { type: 'followOn', seq: 5 }, true, { round: 'keep', goNewest: true, state: state(false, true) }],
    // Position events (ADR 0072, decision 3: `pinned` changes only through these and the table above)
    ['the user lands on the newest end: pinned, the round lets go', state(false, false), { type: 'positioned', atNewest: true, byUser: true }, false, { endHold: true, state: state(false, true) }],
    ['the user moves away from the newest end: unpinned, the round lets go', state(false, true), { type: 'positioned', atNewest: false, byUser: true }, false, { endHold: true, state: state(false, false) }],
    ['the grid moves to the newest end itself (a live write): pinned, nobody took over', state(false, false), { type: 'positioned', atNewest: true, byUser: false }, false, { endHold: false, state: state(false, true) }],
    ['the grid moves away from the newest end itself (a shift): pinned is unchanged', state(false, true), { type: 'positioned', atNewest: false, byUser: false }, true, { endHold: false, state: state(false, true) }],
    ['the grid moves itself to a middle position while not pinned: unchanged', state(true, false), { type: 'positioned', atNewest: false, byUser: false }, false, { endHold: false, state: state(true, false) }],
    // Growth reported by a page outside a round (G9)
    ['following pinned, the total grew, oldest first: write the new end', state(false, true), { type: 'grew' }, false, { growth: 'end', state: state(false, true) }],
    ['following pinned, the total grew, newest first: the view stays at the top', state(false, true), { type: 'grew' }, true, { growth: 'none', state: state(false, true) }],
    ['following not pinned, the total grew, oldest first: the rows are added below: nothing', state(false, false), { type: 'grew' }, false, { growth: 'none', state: state(false, false) }],
    ['following not pinned, the total grew, newest first: rows are inserted above: shift', state(false, false), { type: 'grew' }, true, { growth: 'shift', state: state(false, false) }],
    ['holding and pinned (a revealed view), the total grew, oldest first: write the new end', state(true, true), { type: 'grew' }, false, { growth: 'end', state: state(true, true) }],
    ['holding and not pinned, the total grew: the held rows stay (counted, not shown)', state(true, false), { type: 'grew' }, true, { growth: 'none', state: state(true, false) }],
    ['holding and not pinned, the total grew, oldest first: nothing', state(true, false), { type: 'grew' }, false, { growth: 'none', state: state(true, false) }],
    // The holding bit
    ['holding released without a Follow on (live tail came back, a pause was cleared): following, pinned unchanged', state(true, false), { type: 'released' }, false, { round: 'none', state: state(false, false) }],
    // Any state
    ['a new query or sort order, oldest first: the top is not the newest end', state(false, true), { type: 'reset', seq: 5 }, false, { round: 'none', state: state(false, false) }],
    ['a new query or sort order, newest first: the top is the newest end', state(true, false), { type: 'reset', seq: 5 }, true, { round: 'none', state: state(true, true) }],
    // G1, G2, G9, G10 as one rule: a request stamped before the last take-over (an effect applied it late) is dropped; the user's move wins.
    ['a reveal stamped before the take-over, oldest first: only a reload that keeps the view, not pinned again', state(true, false, 7), { type: 'reveal', seq: 6 }, false, { round: 'keep', goNewest: false, state: state(true, false, 7) }],
    ['a reveal stamped before the take-over, newest first: a shift reload, no move', state(true, false, 7), { type: 'reveal', seq: 6 }, true, { round: 'shift', goNewest: false, state: state(true, false, 7) }],
    ['Follow on stamped before the take-over: no move, not pinned again', state(false, false, 7), { type: 'followOn', seq: 6 }, false, { round: 'keep', goNewest: false, state: state(false, false, 7) }],
    ['a resize stamped before the take-over: dropped, no re-pin', state(false, false, 7), { type: 'resize', seq: 3 }, false, { keepTopRow: false, writeNewest: false, state: state(false, false, 7) }],
    ['a resize stamped before the take-over while the state says pinned: dropped', state(false, true, 7), { type: 'resize', seq: 3 }, false, { writeNewest: false, state: state(false, true, 7) }],
    ['a new query stamped before the take-over: dropped', state(false, true, 7), { type: 'reset', seq: 2 }, false, { round: 'none', state: state(false, true, 7) }],
    ['a reveal stamped after the take-over is applied', state(true, false, 7), { type: 'reveal', seq: 8 }, false, { round: 'end', goNewest: true, state: state(true, true, 7) }],
    ['a resize stamped after the take-over is applied', state(false, true, 7), { type: 'resize', seq: 8 }, false, { writeNewest: true, state: state(false, true, 7) }],
  ];

  it.each(table)('%s', (_name, from, event, descending, expected) => {
    const { state: next, action } = followStep(from, event, descending);
    const { state: nextState, ...actionFields } = expected;
    if (nextState) expect(next).toEqual(nextState);
    expect(action).toMatchObject(actionFields);
  });

  it('data, resize and a hold or release leave `pinned` alone, and only hold, released, followOn and reset touch `holding`', () => {
    for (const event of [{ type: 'data' }, { type: 'resize', seq: 5 }] as const) {
      for (const holding of [true, false]) {
        for (const pinned of [true, false]) {
          for (const descending of [true, false]) expect(followStep(state(holding, pinned), event, descending).state).toEqual(state(holding, pinned));
        }
      }
    }
  });
});
