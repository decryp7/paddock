import {
  formatAxisDuration,
  formatBytes,
  formatCount,
  formatDateTime,
  formatDay,
  formatHour,
  formatDuration,
  formatElapsed,
  formatPermille,
  formatRelative,
  formatShortDate,
  formatTime,
  plural,
  share,
  formatSpan,
} from './format';

describe('formatBytes', () => {
  it('uses binary units and drops decimals for big values', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(10 * 1024 * 1024)).toBe('10.0 MB');
    expect(formatBytes(150 * 1024 ** 3)).toBe('150 GB');
    expect(formatBytes(3 * 1024 ** 5)).toBe('3072 TB');
    expect(formatBytes(1536, 0)).toBe('2 KB');
  });
  it('handles negatives and missing values', () => {
    expect(formatBytes(-2048)).toBe('-2.0 KB');
    expect(formatBytes(null)).toBe('—');
    expect(formatBytes(undefined)).toBe('—');
    expect(formatBytes(Number.NaN)).toBe('—');
    expect(formatBytes(Infinity)).toBe('—');
  });
});

describe('formatDuration', () => {
  it('picks the two largest units', () => {
    expect(formatDuration(42_000)).toBe('42s');
    expect(formatDuration(452_000)).toBe('7m 32s');
    expect(formatDuration(3_900_000)).toBe('1h 05m');
    expect(formatDuration(3 * 86_400_000 + 4 * 3_600_000)).toBe('3d 4h');
  });
  it('clamps negatives and handles missing', () => {
    expect(formatDuration(-5000)).toBe('0s');
    expect(formatDuration(null)).toBe('—');
    expect(formatDuration(undefined)).toBe('—');
    expect(formatDuration(Number.NaN)).toBe('—');
  });
});

describe('formatAxisDuration', () => {
  it('rounds to seconds, minutes or hours', () => {
    expect(formatAxisDuration(45_000)).toBe('45s');
    expect(formatAxisDuration(480_000)).toBe('8 min');
    expect(formatAxisDuration(7_200_000)).toBe('2 h');
    expect(formatAxisDuration(5_400_000)).toBe('1.5 h');
    // Two ticks never read the same: a minute and a bit is not "1 min" again.
    expect(formatAxisDuration(60_000)).toBe('1 min');
    expect(formatAxisDuration(80_000)).toBe('1m 20s');
    expect(formatAxisDuration(100_000)).toBe('1m 40s');
  });
});

describe('formatElapsed', () => {
  it('shows mm:ss or h:mm:ss', () => {
    expect(formatElapsed(452_000)).toBe('07:32');
    expect(formatElapsed(3_600_000 + 7 * 60_000 + 32_000)).toBe('1:07:32');
    expect(formatElapsed(-10)).toBe('00:00');
  });
});

describe('formatRelative', () => {
  const now = 10_000_000_000;
  it('handles missing and near values', () => {
    expect(formatRelative(null, now)).toBe('—');
    expect(formatRelative(undefined, now)).toBe('—');
    expect(formatRelative(now - 10_000, now)).toBe('just now');
    expect(formatRelative(now + 10_000, now)).toBe('in a moment');
  });
  it('picks minutes, hours and days in both directions', () => {
    expect(formatRelative(now - 5 * 60_000, now)).toBe('5 min ago');
    expect(formatRelative(now + 3 * 3_600_000, now)).toBe('in 3 h');
    expect(formatRelative(now - 86_400_000, now)).toBe('1 day ago');
    expect(formatRelative(now - 2 * 86_400_000, now)).toBe('2 days ago');
    expect(formatRelative(now + 3 * 86_400_000, now)).toBe('in 3 days');
  });
});

describe('locale-dependent times', () => {
  const at = new Date(2026, 9, 22, 14, 5).getTime();
  it('formats with the same API as the source', () => {
    expect(formatTime(at)).toBe(new Date(at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' }));
    expect(formatDay(at)).toContain('22');
    expect(formatShortDate(at)).toContain('22');
    expect(formatHour(at)).toMatch(/14[:.]05/);
    expect(formatDateTime(at)).toContain('22');
    expect(formatDateTime(null)).toBe('—');
    expect(formatDateTime(undefined)).toBe('—');
  });
});

describe('formatPermille', () => {
  it('shows one decimal only when it matters', () => {
    expect(formatPermille(420)).toBe('42%');
    expect(formatPermille(998)).toBe('99.8%');
    expect(formatPermille(1000)).toBe('100%');
    expect(formatPermille(55)).toBe('5.5%');
    expect(formatPermille(50)).toBe('5%');
    expect(formatPermille(0)).toBe('0%');
    expect(formatPermille(null)).toBe('—');
    expect(formatPermille(undefined)).toBe('—');
    expect(formatPermille(Number.NaN)).toBe('—');
  });
});

describe('formatCount and plural', () => {
  it('abbreviates large counts', () => {
    expect(formatCount(1284)).toBe('1,284');
    expect(formatCount(12_900)).toBe('12.9K');
    expect(formatCount(20_000)).toBe('20K');
    expect(formatCount(4_200_000)).toBe('4.2M');
  });
  it('pluralises', () => {
    expect(plural(1, 'job')).toBe('1 job');
    expect(plural(3, 'job')).toBe('3 jobs');
    expect(plural(0, 'entry', 'entries')).toBe('0 entries');
    expect(plural(2, 'entry', 'entries')).toBe('2 entries');
  });
});

describe('share', () => {
  it('is clamped and safe for zero totals', () => {
    expect(share(1, 4)).toBe(0.25);
    expect(share(9, 4)).toBe(1);
    expect(share(-1, 4)).toBe(0);
    expect(share(1, 0)).toBe(0);
  });
});

describe('formatSpan', () => {
  it('words a span to the minute', () => {
    expect(formatSpan(null)).toBe('—');
    expect(formatSpan(Number.NaN)).toBe('—');
    expect(formatSpan(10_000)).toBe('under a minute');
    expect(formatSpan(-5)).toBe('under a minute');
    expect(formatSpan(7 * 60_000)).toBe('7 min');
    expect(formatSpan(134 * 60_000)).toBe('2 h 14 min');
    expect(formatSpan(182 * 60_000)).toBe('3 h 02 min');
    expect(formatSpan(180 * 60_000)).toBe('3 h');
    expect(formatSpan(27 * 3_600_000)).toBe('1 d 3 h');
    expect(formatSpan(48 * 3_600_000)).toBe('2 d');
  });
});
