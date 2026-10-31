import { byteAxis, niceAxis } from './axis';

describe('byteAxis', () => {
  const GB = 1024 ** 3;
  it('steps in round numbers of the unit the axis is read in', () => {
    expect(byteAxis(373 * GB)).toEqual({ max: 400 * GB, interval: 100 * GB });
    expect(byteAxis(1204 * GB)).toEqual({ max: 1.5 * 1024 * GB, interval: 0.5 * 1024 * GB });
    expect(byteAxis(90 * GB)).toEqual({ max: 100 * GB, interval: 25 * GB });
  });
  it('survives an empty or odd maximum', () => {
    expect(byteAxis(0).interval).toBeGreaterThan(0);
    expect(byteAxis(Number.NaN).interval).toBeGreaterThan(0);
  });
});

describe('niceAxis', () => {
  it('steps in 1, 2, 2.5 or 5 of a power of ten', () => {
    expect(niceAxis(87)).toEqual({ max: 100, interval: 25 });
    expect(niceAxis(4200)).toEqual({ max: 6000, interval: 2000 });
  });
  it('survives an empty or odd maximum', () => {
    expect(niceAxis(0).interval).toBeGreaterThan(0);
    expect(niceAxis(Number.NaN).interval).toBeGreaterThan(0);
  });
});
