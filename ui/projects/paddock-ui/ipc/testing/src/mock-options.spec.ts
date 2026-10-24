import { describe, expect, it } from 'vitest';
import { formatMockOptions, MAX_MOCK_LATENCY_MS, parseMockOptions } from './mock-options';

describe('parseMockOptions', () => {
  it('reads nothing from an empty query', () => {
    expect(parseMockOptions('')).toEqual({ scenarios: [], latencyMs: 0, fail: {}, hold: [] });
  });
  it('reads scenarios, latency, failures and holds', () => {
    const o = parseMockOptions('?mock=linux,desktop&latency=150&fail=health,save:2&hold=load,export');
    expect(o).toEqual({ scenarios: ['linux', 'desktop'], latencyMs: 150, fail: { health: Infinity, save: 2 }, hold: ['load', 'export'] });
  });
  it('ignores malformed values and caps the latency', () => {
    expect(parseMockOptions('?latency=abc').latencyMs).toBe(0);
    expect(parseMockOptions('?latency=-5').latencyMs).toBe(0);
    expect(parseMockOptions('?latency=999999').latencyMs).toBe(MAX_MOCK_LATENCY_MS);
    expect(parseMockOptions('?fail=a:0,b:x,:3,<script>,ok:1').fail).toEqual({ ok: 1 });
    expect(parseMockOptions('?mock=,,a b,ok&hold=ok,,1x').scenarios).toEqual(['ok']);
    expect(parseMockOptions('?hold=ok,,1x').hold).toEqual(['ok']);
  });
});

describe('formatMockOptions', () => {
  it('writes what parseMockOptions reads', () => {
    const options = { scenarios: ['linux'], latencyMs: 0, fail: { health: Infinity, save: 2 }, hold: ['load'] };
    const text = formatMockOptions(options);
    expect(text).toBe('?mock=linux&latency=0&fail=health,save:2&hold=load');
    expect(parseMockOptions(text)).toEqual(options);
  });
  it('is empty for nothing', () => {
    expect(formatMockOptions({})).toBe('');
  });
});
