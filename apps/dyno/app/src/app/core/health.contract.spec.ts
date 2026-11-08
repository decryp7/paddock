import type { HealthReport } from './api.types';
import wire from './health.wire.json';

/**
 * The wire contract of `HealthReport.notChecked`. `health.wire.json` is written by the Rust test `tests/health_wire.rs` from the
 * engine's own type; the value below is the same one as a TypeScript literal, so the compiler checks it against `api.types.ts`
 * and the test checks it equals what Rust produces.
 */
const report: HealthReport = {
  at: 1,
  score: 100,
  grade: 'good',
  performanceScore: 100,
  cappedBy: null,
  reasons: [],
  notChecked: [
    { what: ['Disk health'], why: 'Needs administrator rights', kind: 'couldNotRead' },
    { what: ['Pending updates'], why: 'Dyno does not check for updates', kind: 'notReadHere' },
    { what: ['Restart needed', 'Startup items'], why: 'Dyno does not check this yet', kind: 'notReadYet' },
  ],
};

describe('the health wire contract with the engine', () => {
  it('has the engine shape of a report with every kind of gap', () => {
    expect(wire).toEqual(report);
  });
});
