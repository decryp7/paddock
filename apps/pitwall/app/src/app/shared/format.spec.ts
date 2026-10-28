import { ago, byteStep, bytes, clock, compact, concurrencyText, countdown, date, duration, gb, int, parseSize, percent, rate, seconds, slotsText, tokensText, wholeBytes } from './format';

interface Table {
  [key: string]: { in: never; out: unknown }[] | string[];
}

async function cases(): Promise<Table> {
  const fs = (await import(/* @vite-ignore */ ['node', 'fs'].join(':'))) as { readFileSync(path: string, encoding: 'utf8'): string };
  return JSON.parse(fs.readFileSync('../../../crates/paddock-units/cases.json', 'utf8')) as Table;
}

describe('format', () => {
  it('follows the case table shared with Rust (crates/paddock-units/cases.json)', async () => {
    const table = await cases();
    const run = (key: string, f: (v: never) => unknown): void => {
      const list = table[key] as { in: never; out: unknown }[];
      expect(list.length).toBeGreaterThan(0);
      for (const c of list) expect(f(c.in), `${key}(${String(c.in)})`).toBe(c.out);
    };
    run('storage', (n: number) => bytes(n, 'storage'));
    run('memory', (n: number) => bytes(n, 'memory'));
    run('wholeStorage', (n: number) => wholeBytes(n, 'storage'));
    run('wholeMemory', (n: number) => wholeBytes(n, 'memory'));
    run('parseStorage', (t: string) => parseSize(t, 'storage'));
    run('parseMemory', (t: string) => parseSize(t, 'memory'));
    for (const bad of table['parseBad'] as string[]) expect(parseSize(bad), bad).toBeNull();
  });

  it('shows missing and negative sizes as a dash and picks the step by kind', () => {
    expect(bytes(null)).toBe('—');
    expect(bytes(undefined)).toBe('—');
    expect(bytes(NaN)).toBe('—');
    expect(bytes(-5)).toBe('—');
    expect(byteStep('storage')).toBe(1000);
    expect(byteStep('memory')).toBe(1024);
    expect(bytes(16 * 1024 ** 3, 'memory')).toBe('16 GB');
  });

  it('gives GB figures and percentages', () => {
    expect(gb(17.4 * 1000 ** 3, 'storage')).toBe('17.4');
    expect(gb(32 * 1024 ** 3, 'memory', 0)).toBe('32');
    expect(gb(null, 'memory')).toBe('—');
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
});
