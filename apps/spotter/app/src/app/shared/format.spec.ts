import { bucketLabel, formatBytes, formatDate, formatDateTime, formatDuration, formatInterval, lastSegment, relativeTime, shortHash, spanText } from './format';

const MIN = 60_000;
const H = 60 * MIN;
const D = 24 * H;

describe('format', () => {
  it('writes spans in the unit that fits', () => {
    expect(spanText(10_000)).toBe('10 s');
    expect(spanText(5 * MIN)).toBe('5 min');
    expect(spanText(3 * H)).toBe('3 h');
    expect(spanText(2 * D)).toBe('2 days');
    expect(spanText(15 * D)).toBe('2 weeks');
    expect(spanText(90 * D)).toBe('3 months');
    expect(spanText(800 * D)).toBe('2 years');
    expect(spanText(-3 * H)).toBe('3 h');
    expect(spanText(7 * D)).toBe('7 days');
    expect(spanText(365 * D)).toBe('12 months');
  });

  it('writes relative times both ways', () => {
    const now = 1_000_000_000;
    expect(relativeTime(null, now)).toBe('—');
    expect(relativeTime(undefined, now)).toBe('—');
    expect(relativeTime(now - 10_000, now)).toBe('just now');
    expect(relativeTime(now + 10_000, now)).toBe('in a moment');
    expect(relativeTime(now - 3 * MIN, now)).toBe('3 min ago');
    expect(relativeTime(now + 2 * H, now)).toBe('in 2 h');
  });

  it('formats durations, intervals and sizes', () => {
    expect(formatDuration(850)).toBe('850 ms');
    expect(formatDuration(2400)).toBe('2.4 s');
    expect(formatDuration(3 * MIN)).toBe('3 min');
    expect(formatInterval(6 * 3600)).toBe('every 6 h');
    expect(formatBytes(null)).toBe('—');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.5 KiB');
    expect(formatBytes(150 * 1024 * 1024)).toBe('150 MiB');
    expect(formatBytes(5 * 1024 ** 4 * 2000)).toContain('TiB');
  });

  it('formats dates without depending on the time zone', () => {
    expect(formatDateTime(null)).toBe('—');
    expect(formatDate(undefined)).toBe('—');
    expect(formatDateTime(Date.UTC(2026, 5, 15, 12))).toContain('2026');
    expect(formatDate(Date.UTC(2026, 5, 15, 12))).toContain('2026');
    expect(bucketLabel(Date.UTC(2026, 5, 15, 12))).toMatch(/Jun/);
  });

  it('shortens hashes and paths', () => {
    expect(shortHash(null)).toBe('—');
    expect(shortHash('abcdef0123456789', 6)).toBe('abcdef');
    expect(lastSegment('https://x.test/a/b/')).toBe('b');
    expect(lastSegment('C:\\drops\\acme')).toBe('acme');
    expect(lastSegment('')).toBe('');
  });
});
