import { LIVE_CONVERSATION } from '../generated/live-fixtures';
import { foldFrame, initialFolded, parseFrame, parseLink, sameLink, type LiveFolded, type LiveFrame, type LiveLink } from './fold';

interface Overview {
  memory: number;
  events: string[];
  count: number;
}
type Change = { kind: 'memory'; bytes: number } | { kind: 'event'; text: string } | { kind: 'bump' } | { kind: 'explode' };

const empty: Overview = { memory: 0, events: [], count: 0 };

/** The same fold as `Overview::apply` in Rust (`paddock-live/tests/common`). */
function apply(s: Overview, d: Change): Overview {
  switch (d.kind) {
    case 'memory':
      return { ...s, memory: d.bytes };
    case 'event':
      return { ...s, events: [...s.events, d.text] };
    case 'bump':
      return { ...s, count: s.count + 1 };
    case 'explode':
      throw new Error('unreachable in the UI');
  }
}

const connected: LiveLink = { state: 'connected' };
const snapshot = (seq: number, state: Overview = empty, link: LiveLink = connected): LiveFrame<Overview, Change> => ({ kind: 'snapshot', seq, state, link });
const delta = (from: number, seq: number, d: Change): LiveFrame<Overview, Change> => ({ kind: 'delta', from, seq, delta: d });
const at = (seq: number, state: Overview = empty): LiveFolded<Overview> => ({ state, seq, link: connected });

describe('foldFrame', () => {
  it('starts connecting with the empty state at sequence 0', () => {
    expect(initialFolded(empty)).toEqual({ state: empty, seq: 0, link: { state: 'connecting' } });
  });

  it('replaces everything with a snapshot, whatever its number when it was pushed', () => {
    const r = foldFrame(at(50), snapshot(3, { ...empty, count: 9 }), apply, 'event');
    expect(r.outcome).toBe('applied');
    expect(r.folded).toEqual({ state: { ...empty, count: 9 }, seq: 3, link: connected });
  });

  it('ignores a fetched snapshot that is older than the window, and takes an equal or newer one', () => {
    const cur = at(10);
    const stale = foldFrame(cur, snapshot(9), apply, 'fetched');
    expect(stale.outcome).toBe('stale');
    expect(stale.folded).toBe(cur);
    expect(foldFrame(cur, snapshot(10), apply, 'fetched').outcome).toBe('applied');
    expect(foldFrame(cur, snapshot(11), apply, 'fetched').folded.seq).toBe(11);
  });

  it('applies the next delta and keeps the link', () => {
    const r = foldFrame(at(4), delta(5, 5, { kind: 'bump' }), apply);
    expect(r.outcome).toBe('applied');
    expect(r.folded.seq).toBe(5);
    expect(r.folded.state.count).toBe(1);
    expect(r.folded.link).toBe(connected);
  });

  it('applies a merged delta that covers the next number, even one that overlaps what the window has', () => {
    expect(foldFrame(at(4), delta(5, 8, { kind: 'memory', bytes: 7 }), apply).folded).toMatchObject({ seq: 8, state: { memory: 7 } });
    expect(foldFrame(at(6), delta(5, 8, { kind: 'memory', bytes: 7 }), apply).folded).toMatchObject({ seq: 8, state: { memory: 7 } });
  });

  it('drops a delta it already has', () => {
    const cur = at(5);
    const r = foldFrame(cur, delta(5, 5, { kind: 'bump' }), apply);
    expect(r.outcome).toBe('duplicate');
    expect(r.folded).toBe(cur);
    expect(foldFrame(cur, delta(2, 4, { kind: 'bump' }), apply).outcome).toBe('duplicate');
  });

  it('reports a gap and changes nothing', () => {
    const cur = at(5);
    const r = foldFrame(cur, delta(7, 7, { kind: 'bump' }), apply);
    expect(r.outcome).toBe('gap');
    expect(r.folded).toBe(cur);
  });

  it('replaces the link, and returns the same object when it did not change', () => {
    const cur = at(5);
    const down: LiveLink = { state: 'down', reason: 'no agent' };
    const r = foldFrame(cur, { kind: 'link', link: down }, apply);
    expect(r.outcome).toBe('applied');
    expect(r.folded).toEqual({ ...cur, link: down });
    const same = foldFrame(cur, { kind: 'link', link: { state: 'connected' } }, apply);
    expect(same.outcome).toBe('duplicate');
    expect(same.folded).toBe(cur);
  });

  it('compares links by state and details', () => {
    const r1: LiveLink = { state: 'reconnecting', attempt: 1, retryInMs: 1000, reason: 'closed' };
    expect(sameLink(r1, { ...r1 })).toBe(true);
    expect(sameLink(r1, { ...r1, attempt: 2 })).toBe(false);
    expect(sameLink(r1, { ...r1, retryInMs: 2000 })).toBe(false);
    expect(sameLink(r1, { ...r1, reason: 'x' })).toBe(false);
    expect(sameLink({ state: 'down', reason: 'a' }, { state: 'down', reason: 'a' })).toBe(true);
    expect(sameLink({ state: 'down', reason: 'a' }, { state: 'down', reason: 'b' })).toBe(false);
    expect(sameLink({ state: 'connected' }, { state: 'stopped' })).toBe(false);
    expect(sameLink({ state: 'stopped' }, { state: 'stopped' })).toBe(true);
  });
});

describe('the conversation Rust sent', () => {
  it('folds to the state, sequence and link Rust ended with (the two folds are equal)', () => {
    let cur = initialFolded(empty);
    for (const raw of LIVE_CONVERSATION.frames) {
      const frame = parseFrame<Overview, Change>(raw);
      expect(frame, JSON.stringify(raw)).not.toBeNull();
      const r = foldFrame(cur, frame as LiveFrame<Overview, Change>, apply);
      expect(r.outcome).not.toBe('gap');
      cur = r.folded;
    }
    expect(cur.state).toEqual(LIVE_CONVERSATION.finalState);
    expect(cur.seq).toBe(LIVE_CONVERSATION.finalSeq);
    expect(cur.link).toEqual(LIVE_CONVERSATION.finalLink);
  });

  it('contains every frame shape: a snapshot, a plain delta, a merged delta and link changes', () => {
    const frames = LIVE_CONVERSATION.frames.map((f) => parseFrame<Overview, Change>(f));
    expect(frames.some((f) => f?.kind === 'snapshot')).toBe(true);
    expect(frames.some((f) => f?.kind === 'delta' && f.from === f.seq)).toBe(true);
    expect(frames.some((f) => f?.kind === 'delta' && f.from < f.seq)).toBe(true);
    expect(frames.some((f) => f?.kind === 'link' && f.link.state === 'down')).toBe(true);
  });
});

describe('parsing at the edge', () => {
  it.each([
    ['null', null],
    ['an array', []],
    ['a string', 'snapshot'],
    ['an unknown kind', { kind: 'other' }],
    ['a snapshot without a state', { kind: 'snapshot', seq: 1, link: { state: 'connected' } }],
    ['a snapshot with a bad seq', { kind: 'snapshot', seq: -1, state: {}, link: { state: 'connected' } }],
    ['a snapshot with a fractional seq', { kind: 'snapshot', seq: 1.5, state: {}, link: { state: 'connected' } }],
    ['a snapshot with a bad link', { kind: 'snapshot', seq: 1, state: {}, link: { state: 'sideways' } }],
    ['a delta without a delta', { kind: 'delta', from: 1, seq: 1 }],
    ['a delta that ends before it starts', { kind: 'delta', from: 3, seq: 2, delta: {} }],
    ['a delta with a string seq', { kind: 'delta', from: 1, seq: '1', delta: {} }],
    ['a link frame without a link', { kind: 'link' }],
  ])('rejects %s', (_name, raw) => {
    expect(parseFrame(raw)).toBeNull();
  });

  it('accepts the three shapes', () => {
    expect(parseFrame({ kind: 'snapshot', seq: 0, state: { a: 1 }, link: { state: 'connecting' } })).toEqual({ kind: 'snapshot', seq: 0, state: { a: 1 }, link: { state: 'connecting' } });
    expect(parseFrame({ kind: 'delta', from: 2, seq: 4, delta: 'x' })).toEqual({ kind: 'delta', from: 2, seq: 4, delta: 'x' });
    expect(parseFrame({ kind: 'link', link: { state: 'stopped' } })).toEqual({ kind: 'link', link: { state: 'stopped' } });
  });

  it('reads every link shape and refuses the incomplete ones', () => {
    expect(parseLink({ state: 'reconnecting', attempt: 2, retryInMs: 2000, reason: 'closed' })).toEqual({ state: 'reconnecting', attempt: 2, retryInMs: 2000, reason: 'closed' });
    expect(parseLink({ state: 'down', reason: 'gave up' })).toEqual({ state: 'down', reason: 'gave up' });
    expect(parseLink({ state: 'connected' })).toEqual({ state: 'connected' });
    expect(parseLink({ state: 'reconnecting', attempt: 'two', retryInMs: 1, reason: 'x' })).toBeNull();
    expect(parseLink({ state: 'reconnecting', attempt: 1, retryInMs: 1 })).toBeNull();
    expect(parseLink({ state: 'down' })).toBeNull();
    expect(parseLink({})).toBeNull();
    expect(parseLink(7)).toBeNull();
  });
});
