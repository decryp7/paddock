import { ago, bytes, bytesIn, bytesUnit, clock, compact, concurrencyText, countdown, date, duration, durationIn, durationUnit, gb, int, percent, rate, seconds, slotsText, tokensText, varies } from './format';

const GB = 1024 ** 3;

describe('format', () => {
  it('formats bytes in binary units', () => {
    expect(bytes(0)).toBe('0 B');
    expect(bytes(512)).toBe('512 B');
    expect(bytes(1536)).toBe('1.5 KB');
    expect(bytes(5.03 * GB)).toBe('5.0 GB');
    expect(bytes(262 * 1024 ** 2)).toBe('262 MB');
    expect(bytes(3 * 1024 ** 5)).toBe('3072 TB');
    expect(bytes(null)).toBe('—');
    expect(bytes(undefined)).toBe('—');
    expect(bytes(NaN)).toBe('—');
    expect(bytes(-5)).toBe('0 B');
  });

  it('gives GB figures and percentages', () => {
    expect(gb(17.4 * GB)).toBe('17.4');
    expect(gb(32 * GB, 0)).toBe('32');
    expect(gb(null)).toBe('—');
    expect(percent(1, 4)).toBe(25);
    expect(percent(5, 0)).toBe(0);
    expect(percent(9, 4)).toBe(100);
  });

  it('formats durations and settings seconds', () => {
    expect(duration(820)).toBe('820 ms');
    expect(duration(1_400)).toBe('1.4 s');
    expect(duration(42_000)).toBe('42 s');
    expect(duration(3 * 60_000)).toBe('3 min');
    expect(duration(125 * 60_000)).toBe('2 h 5 min');
    expect(duration(120 * 60_000)).toBe('2 h');
    expect(duration(72 * 3_600_000)).toBe('3 d');
    expect(duration(null)).toBe('—');
    expect(seconds(45)).toBe('45 s');
    expect(seconds(600)).toBe('10 min');
    expect(seconds(90)).toBe('1 min 30 s');
    expect(seconds(7200)).toBe('2 h');
    expect(seconds(5400)).toBe('1 h 30 min');
  });

  it('counts down and tells relative time', () => {
    expect(countdown(10_000 + 252_000, 10_000)).toBe('4:12');
    expect(countdown(0, 5_000)).toBe('0:00');
    expect(countdown(3_725_000, 0)).toBe('1:02:05');
    expect(ago(1000, 1000 + 10_000)).toBe('just now');
    expect(ago(0, 5 * 60_000)).toBe('5 min ago');
    expect(ago(5 * 60_000, 0)).toBe('in 5 min');
    expect(ago(null, 0)).toBe('never');
  });

  it('formats clock times, dates, counts and rates', () => {
    const at = new Date(2026, 9, 22, 9, 5, 7).getTime();
    expect(clock(at)).toBe('09:05');
    expect(clock(at, true)).toBe('09:05:07');
    expect(date(at)).toBe('22 Oct');
    expect(date(at, true)).toBe('22 Oct 09:05');
    expect(compact(950)).toBe('950');
    expect(compact(1_234)).toBe('1.2k');
    expect(compact(12_345)).toBe('12k');
    expect(compact(4_100_000)).toBe('4.1M');
    expect(compact(41_000_000)).toBe('41M');
    expect(compact(2_500_000_000)).toBe('2.5B');
    expect(compact(null)).toBe('—');
    expect(rate(42.66)).toBe('42.7');
    expect(rate(1234.4)).toBe('1234');
    expect(rate(undefined)).toBe('—');
    expect(int(12345.6)).toBe('12,346');
    expect(int(null)).toBe('—');
  });

  it('words slots, concurrency and tokens', () => {
    expect(slotsText(1)).toBe('1 slot');
    expect(slotsText(4)).toBe('4 slots');
    expect(concurrencyText(0)).toBe('does not fit even one request');
    expect(concurrencyText(1)).toBe('one request at a time');
    expect(concurrencyText(6)).toBe('up to 6 requests at once');
    expect(tokensText(8192)).toBe('8,192 tokens');
    expect(tokensText(1)).toBe('1 token');
  });
  it('puts a column of bytes in the unit its largest value reads best in', () => {
    const G = 1024 ** 3;
    expect(bytesUnit([5.6 * G, 600 * 1024 ** 2, null])).toBe('GB');
    expect(bytesUnit([300 * 1024 ** 2])).toBe('MB');
    expect(bytesUnit([])).toBe('B');
    expect(bytesUnit([NaN, 512])).toBe('B');
    expect(bytesIn(5.6 * G, 'GB')).toBe('5.6');
    expect(bytesIn(600 * 1024 ** 2, 'GB')).toBe('0.6');
    expect(bytesIn(150 * G, 'GB')).toBe('150');
    expect(bytesIn(700, 'B')).toBe('700');
    expect(bytesIn(null, 'GB')).toBe('—');
  });

  it('puts a column of durations in one unit', () => {
    expect(durationUnit([955, 2200])).toBe('ms');
    expect(durationUnit([12_000, null])).toBe('s');
    expect(durationUnit([45 * 60_000])).toBe('min');
    expect(durationUnit([11 * 3_600_000])).toBe('h');
    expect(durationUnit([])).toBe('ms');
    expect(durationIn(2213, 'ms')).toBe('2,213');
    expect(durationIn(12_340, 's')).toBe('12.3');
    expect(durationIn(150_000, 's')).toBe('150');
    expect(durationIn(48 * 60_000 + 20_000, 'min')).toBe('48');
    expect(durationIn(11.5 * 3_600_000, 'h')).toBe('11.5');
    expect(durationIn(-5, 'ms')).toBe('0');
    expect(durationIn(undefined, 's')).toBe('—');
  });

  it('tells a column that varies from a constant one', () => {
    expect(varies([{ a: 1 }, { a: 1 }], (r) => r.a)).toBe(false);
    expect(varies([{ a: 1 }, { a: 2 }], (r) => r.a)).toBe(true);
    expect(varies([{ a: 1 }], (r) => r.a)).toBe(true);
    expect(varies([], () => 0)).toBe(true);
  });
});
