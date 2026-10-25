import { agoShort, bucketLabel, formatShortDateTime, mebibytes, formatBytes, formatDate, formatDateTime, formatDuration, formatInterval, lastSegment, relativeTime, shortHash, spanText } from './format';

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

describe('short formats', () => {
  const NOW = new Date(2026, 9, 22, 16, 30).getTime();

  it('writes unknown values as a dash', () => {
    expect(formatShortDateTime(null, NOW)).toBe('—');
    expect(formatShortDateTime(undefined, NOW)).toBe('—');
    expect(mebibytes(null)).toBe('—');
    expect(mebibytes(undefined)).toBe('—');
    expect(agoShort(null, NOW)).toBe('—');
    expect(agoShort(undefined, NOW)).toBe('—');
  });

  it('shows the time this year and only the date in other years', () => {
    const thisYear = new Date(2026, 9, 22, 16, 20).getTime();
    expect(formatShortDateTime(thisYear, NOW)).toMatch(/Oct/);
    expect(formatShortDateTime(thisYear, NOW)).toMatch(/4:20|16:20/);
    const old = new Date(2024, 2, 5, 8, 15).getTime();
    expect(formatShortDateTime(old, NOW)).toContain('2024');
    expect(formatShortDateTime(old, NOW)).not.toMatch(/8:15|08:15/);
  });

  it('scales mebibytes to keep three significant digits', () => {
    expect(mebibytes(0)).toBe('0.00');
    expect(mebibytes(1048576 * 25.5)).toBe('25.5');
    expect(mebibytes(1048576 * 5.25)).toBe('5.25');
    expect(mebibytes(1048576 * 250)).toBe('250');
  });

  it('writes a compact ago', () => {
    expect(agoShort(NOW - 10_000, NOW)).toBe('now');
    expect(agoShort(NOW + 10_000, NOW)).toBe('now');
    expect(agoShort(NOW - 20 * MIN, NOW)).toBe('20 min');
    expect(agoShort(NOW - 3 * H, NOW)).toBe('3 h');
    expect(agoShort(NOW - 2 * D, NOW)).toBe('2 days');
  });
});
