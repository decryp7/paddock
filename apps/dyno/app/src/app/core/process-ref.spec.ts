import { identify, isSame, notDifferent, refKey, refOf } from './process-ref';

describe('process references (rule 53)', () => {
  it('the same pid is the same process only when the start times say so', () => {
    expect(identify({ pid: 1, started: 5 }, { pid: 1, started: 5 })).toBe('same');
    expect(identify({ pid: 1, started: 5 }, { pid: 1, started: 6 })).toBe('different');
    expect(identify({ pid: 1, started: 5 }, { pid: 2, started: 5 })).toBe('different');
  });
  it('a start time missing on either side is unknown, never same', () => {
    expect(identify({ pid: 1, started: null }, { pid: 1, started: null })).toBe('unknown');
    expect(identify({ pid: 1, started: 5 }, { pid: 1, started: null })).toBe('unknown');
    expect(identify({ pid: 1, started: null }, { pid: 2, started: null })).toBe('different');
  });
  it('a screen may keep what it holds unless the references show another process', () => {
    expect(notDifferent({ pid: 1, started: 5 }, { pid: 1, started: 5 })).toBe(true);
    expect(notDifferent({ pid: 1, started: null }, { pid: 1, started: 5 })).toBe(true);
    expect(notDifferent({ pid: 1, started: 5 }, { pid: 1, started: 6 })).toBe(false);
  });
  it('a selection or a reply is the same process only on the same start time, or when neither side ever had one', () => {
    expect(isSame({ pid: 1, started: 5 }, { pid: 1, started: 5 })).toBe(true);
    expect(isSame({ pid: 1, started: null }, { pid: 1, started: null })).toBe(true);
    expect(isSame({ pid: 1, started: null }, { pid: 1, started: 5 })).toBe(false);
    expect(isSame({ pid: 1, started: 5 }, { pid: 1, started: 6 })).toBe(false);
  });
  it('keys carry the start time, and the sample gives its reference', () => {
    expect(refKey({ pid: 7, started: 99 })).toBe('p7@99');
    expect(refKey({ pid: 7, started: null })).toBe('p7');
    expect(refOf({ pid: 7, started: { value: 99 } })).toEqual({ pid: 7, started: 99 });
    expect(refOf({ pid: 7, started: { unavailable: { reason: 'notPermitted' } } })).toEqual({ pid: 7, started: null });
  });
});
