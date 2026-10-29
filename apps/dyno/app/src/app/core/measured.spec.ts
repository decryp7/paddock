import fixture from '../../../../contract/measured-wire.json';
import type { Measured, OtherUsers, ProcessSample, UnavailableReason } from './api.types';
import { ALL_REASONS, measured, readReason, readValue, reasonText, unavailable, unavailableText, unreadableShort, unreadableText, UNAVAILABLE } from './measured';

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

  it('matches the shared wire fixture the Rust core reads and writes (contract/measured-wire.json)', () => {
    const wire = fixture.process as ProcessSample;
    const cpu: Measured<number> = wire.cpu;
    expect(readReason(cpu)).toBe('needsAdmin');
    expect(readReason(wire.diskWriteBps)).toBe('notPermitted');
    expect(readValue(wire.memory)).toBe(5);
    expect(readValue(wire.started)).toBe(2000);
    expect(readValue(wire.user)).toBe('me');
    expect(wire.owner).toBe('other');
    expect(fixture.reasons).toEqual([...ALL_REASONS]);
    const other = fixture.otherUsers as OtherUsers;
    expect([other.count, other.partial, readReason(other.memory)]).toEqual([3, true, 'needsAdmin']);
  });
});

describe('unreadable folders', () => {
  it('say why, in words the user can act on, and fall back to a plain reason', () => {
    expect(unreadableText('needsFullDiskAccess')).toBe('Unavailable: Needs Full Disk Access: allow Dyno in System Settings › Privacy & Security › Full Disk Access');
    expect(unreadableText('protected')).toBe('Unavailable: macOS protects this folder');
    expect(unreadableShort('needsFullDiskAccess')).toBe('Needs Full Disk Access');
    expect(unreadableShort(null)).toBe('Could not be read');
    expect(unreadableText('needsAdmin')).toBe('Unavailable: Needs administrator rights to read');
    expect(unreadableText('other')).toBe('Unavailable: The system could not read this folder');
    expect(unreadableText(null)).toBe(unreadableText('other'));
  });
});
