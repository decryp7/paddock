import {
  formatAffinity,
  formatBytes,
  formatBytesDelta,
  formatClock,
  formatCount,
  formatDate,
  formatDayTime,
  formatDuration,
  formatFraction,
  formatGhz,
  formatPercent,
  formatRate,
  formatRelative,
  formatSeconds,
  formatSigned,
  plural,
} from './format';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const NOW = 1_700_000_000_000;

describe('formatBytes', () => {
  it('uses binary multiples', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(1024 ** 2)).toBe('1 MB');
    expect(formatBytes(12.34 * 1024 ** 2)).toBe('12.3 MB');
    expect(formatBytes(123.4 * 1024 ** 3)).toBe('123 GB');
  });
  it('promotes values that would print as 1000 of a unit', () => {
    expect(formatBytes(1000 * 1024)).toBe('0.98 MB');
    expect(formatBytes(1000)).toBe('0.98 KB');
  });
  it('caps at PB and handles negatives', () => {
    expect(formatBytes(2 * 1024 ** 6)).toBe('2048 PB');
    expect(formatBytes(-2048)).toBe('−2 KB');
  });
  it('shows a dash for missing or non-finite values', () => {
    for (const v of [null, undefined, NaN, Infinity]) expect(formatBytes(v)).toBe('—');
  });
  it('formats deltas with an explicit sign', () => {
    expect(formatBytesDelta(1024)).toBe('+1 KB');
    expect(formatBytesDelta(-1024)).toBe('−1 KB');
    expect(formatBytesDelta(0)).toBe('0 B');
  });
});

describe('formatRate', () => {
  it('appends /s and clamps negatives', () => {
    expect(formatRate(2.4 * 1024 ** 2)).toBe('2.4 MB/s');
    expect(formatRate(-5)).toBe('0 B/s');
    expect(formatRate(null)).toBe('—');
    expect(formatRate(NaN)).toBe('—');
    expect(formatRate(undefined)).toBe('—');
  });
});

describe('percentages', () => {
  it('formatPercent picks a sensible precision', () => {
    expect(formatPercent(0.4)).toBe('0.4%');
    expect(formatPercent(5)).toBe('5%');
    expect(formatPercent(5.5)).toBe('5.5%');
    expect(formatPercent(12.4)).toBe('12%');
    expect(formatPercent(0)).toBe('0%');
    expect(formatPercent(100)).toBe('100%');
    expect(formatPercent(null)).toBe('—');
    expect(formatPercent(undefined)).toBe('—');
    expect(formatPercent(Infinity)).toBe('—');
  });
  it('formatPercent honours fixed digits', () => {
    expect(formatPercent(12.345, 1)).toBe('12.3%');
    expect(formatPercent(3, 0)).toBe('3%');
  });
  it('formatFraction scales 0-1', () => {
    expect(formatFraction(0.87)).toBe('87%');
    expect(formatFraction(null)).toBe('—');
    expect(formatFraction(undefined)).toBe('—');
    expect(formatFraction(NaN)).toBe('—');
  });
});

describe('formatSigned', () => {
  it('signs, rounds and handles zero', () => {
    expect(formatSigned(3)).toBe('+3');
    expect(formatSigned(-2.4, ' pts')).toBe('−2 pts');
    expect(formatSigned(0.2)).toBe('±0');
    expect(formatSigned(0, '%')).toBe('±0%');
  });
});

describe('durations', () => {
  it('formatDuration uses two units at most', () => {
    expect(formatDuration(0)).toBe('0 s');
    expect(formatDuration(45)).toBe('45 s');
    expect(formatDuration(60)).toBe('1 min');
    expect(formatDuration(200)).toBe('3 min 20 s');
    expect(formatDuration(2 * 3600)).toBe('2 h');
    expect(formatDuration(2 * 3600 + 5 * 60)).toBe('2 h 5 min');
    expect(formatDuration(3 * 86400)).toBe('3 d');
    expect(formatDuration(3 * 86400 + 4 * 3600)).toBe('3 d 4 h');
  });
  it('formatDuration rejects bad input', () => {
    for (const v of [null, undefined, NaN, -1, Infinity]) expect(formatDuration(v)).toBe('—');
  });
  it('formatSeconds keeps a decimal for short timings', () => {
    expect(formatSeconds(12.4)).toBe('12.4 s');
    expect(formatSeconds(12)).toBe('12 s');
    expect(formatSeconds(90)).toBe('1 min 30 s');
    expect(formatSeconds(null)).toBe('—');
    expect(formatSeconds(NaN)).toBe('—');
    expect(formatSeconds(undefined)).toBe('—');
  });
});

describe('formatRelative', () => {
  it('reads the past', () => {
    expect(formatRelative(NOW - 10_000, NOW)).toBe('just now');
    expect(formatRelative(NOW - 5 * MIN, NOW)).toBe('5 min ago');
    expect(formatRelative(NOW - 3 * HOUR, NOW)).toBe('3 h ago');
    expect(formatRelative(NOW - DAY, NOW)).toBe('yesterday');
    expect(formatRelative(NOW - 4 * DAY, NOW)).toBe('4 days ago');
    expect(formatRelative(NOW - 35 * DAY, NOW)).toBe('1 month ago');
    expect(formatRelative(NOW - 100 * DAY, NOW)).toBe('3 months ago');
  });
  it('reads the future', () => {
    expect(formatRelative(NOW + 10_000, NOW)).toBe('in a moment');
    expect(formatRelative(NOW + 5 * MIN, NOW)).toBe('in 5 min');
    expect(formatRelative(NOW + 3 * HOUR, NOW)).toBe('in 3 h');
    expect(formatRelative(NOW + DAY, NOW)).toBe('tomorrow');
    expect(formatRelative(NOW + 3 * DAY, NOW)).toBe('in 3 days');
  });
});

describe('dates and clock', () => {
  it('formatClock is HH:MM in the local zone', () => {
    const at = new Date(2024, 0, 15, 14, 5).getTime();
    expect(formatClock(at)).toMatch(/14\D05/);
  });
  it('formatDayTime includes the weekday and the time', () => {
    const at = new Date(2024, 0, 15, 9, 7).getTime(); // a Monday
    const s = formatDayTime(at);
    expect(s).toMatch(/Mon/);
    expect(s).toMatch(/09\D07/);
  });
  it('formatDate omits the year within the same year only', () => {
    const at = new Date(2024, 9, 12, 12).getTime();
    const sameYear = new Date(2024, 11, 1, 12).getTime();
    const nextYear = new Date(2025, 5, 1, 12).getTime();
    expect(formatDate(at, sameYear)).toMatch(/12/);
    expect(formatDate(at, sameYear)).not.toMatch(/2024/);
    expect(formatDate(at, nextYear)).toMatch(/2024/);
    expect(formatDate(at)).not.toMatch(/2024/);
  });
});

describe('misc formatters', () => {
  it('formatGhz converts MHz', () => {
    expect(formatGhz(3420)).toBe('3.42 GHz');
    for (const v of [0, -5, null, undefined, NaN]) expect(formatGhz(v)).toBe('—');
  });
  it('formatCount groups digits', () => {
    expect(formatCount(0)).toBe('0');
    expect(formatCount(12345.6)).toMatch(/^12\D?346$/);
    for (const v of [null, undefined, NaN]) expect(formatCount(v)).toBe('—');
  });
  it('plural picks the form', () => {
    expect(plural(1, 'process', 'processes')).toBe('1 process');
    expect(plural(3, 'process', 'processes')).toBe('3 processes');
    expect(plural(0, 'file')).toBe('0 files');
    expect(plural(1, 'file')).toBe('1 file');
  });
});

describe('formatAffinity', () => {
  it('describes masks', () => {
    expect(formatAffinity(null, 4)).toBe('—');
    expect(formatAffinity(undefined, 4)).toBe('—');
    expect(formatAffinity(0, 4)).toBe('none');
    expect(formatAffinity(0b1111, 4)).toBe('all 4 cores');
    expect(formatAffinity(0b0111, 8)).toBe('0–2');
    expect(formatAffinity(0b1011, 4)).toBe('0, 1, 3');
    expect(formatAffinity(0b1, 4)).toBe('0');
    expect(formatAffinity(0b110, 8)).toBe('1, 2');
    expect(formatAffinity(0b10100101, 8)).toBe('0, 2, 5, 7');
    expect(formatAffinity(0b11011110, 8)).toBe('1–4, 6, 7');
  });
});

describe('formatMetric', () => {
  it('keeps digits proportional to size', async () => {
    const { formatMetric } = await import('./format');
    expect(formatMetric(2410.4)).toMatch(/2.?410/);
    expect(formatMetric(612.3)).toBe('612');
    expect(formatMetric(58.44)).toBe('58.4');
    expect(formatMetric(0.923)).toBe('0.92');
    expect(formatMetric(NaN)).toBe('—');
    expect(formatMetric(null)).toBe('—');
  });
});

describe('formatSpan', () => {
  it('reads a span of days as days, weeks, months or years', async () => {
    const { formatSpan } = await import('./format');
    expect(formatSpan(1)).toBe('1 day');
    expect(formatSpan(9)).toBe('9 days');
    expect(formatSpan(30)).toBe('4 weeks');
    expect(formatSpan(520)).toBe('17 months');
    expect(formatSpan(690)).toBe('23 months');
    expect(formatSpan(940)).toBe('31 months');
    expect(formatSpan(2700)).toBe('7.4 years');
    expect(formatSpan(3650)).toBe('10 years');
    expect(formatSpan(1100)).toBe('3 years');
    expect(formatSpan(1000)).toBe('33 months');
    expect(formatSpan(-1)).toBe('—');
    expect(formatSpan(NaN)).toBe('—');
    expect(formatSpan(null)).toBe('—');
  });
});

describe('formatStamp', () => {
  it('drops the year only when it is the current one', async () => {
    const { formatStamp } = await import('./format');
    const now = Date.UTC(2026, 9, 22, 14, 30);
    expect(formatStamp(Date.UTC(2026, 9, 20, 12, 0), now)).not.toContain('2026');
    expect(formatStamp(Date.UTC(2025, 9, 20, 12, 0), now)).toContain('2025');
    expect(formatStamp(now)).not.toContain('2026');
  });
});
