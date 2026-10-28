import type { Measured, ProcessSample, UnavailableReason } from './api.types';
import { measured, readReason, readValue, reasonText, unavailable, unavailableText, UNAVAILABLE } from './measured';

describe('measured values', () => {
  it('reads a value and never turns an unread one into 0', () => {
    expect(readValue(measured(0))).toBe(0);
    expect(readValue(unavailable<number>('needsAdmin'))).toBeNull();
    expect(readReason(measured(1))).toBeNull();
    expect(readReason(unavailable<number>('notSupported'))).toBe('notSupported');
  });

  it('words every reason and prefixes the tooltip with Unavailable', () => {
    const reasons: UnavailableReason[] = ['needsAdmin', 'notPermitted', 'notSupported', 'residentMemoryOnly'];
    for (const r of reasons) {
      expect(reasonText(r).length).toBeGreaterThan(5);
      expect(unavailableText(unavailable<number>(r))).toBe(`${UNAVAILABLE}: ${reasonText(r)}`);
    }
    expect(unavailableText(measured(3))).toBeNull();
    expect(reasonText('needsAdmin')).toContain('administrator');
    expect(reasonText('residentMemoryOnly')).toBe('Linux gives Dyno only resident memory for these');
  });

  it('matches the JSON the Rust core sends (the same literals are asserted in dyno-core process.rs)', () => {
    const wire = JSON.parse(
      '{"pid":2,"parent":1,"name":"b.exe","exe":null,"user":{"value":"me"},"cpu":{"unavailable":{"reason":"needsAdmin"}},' +
        '"memory":{"value":5},"diskReadBps":{"value":0},"diskWriteBps":{"value":0},"started":{"value":2000},"state":"running"}',
    ) as ProcessSample;
    const cpu: Measured<number> = wire.cpu;
    expect(readReason(cpu)).toBe('needsAdmin');
    expect(readValue(wire.memory)).toBe(5);
    expect(readValue(wire.started)).toBe(2000);
  });
});
