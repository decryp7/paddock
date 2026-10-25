import { afterEach, describe, expect, it } from 'vitest';
import { PSEUDO_LOCALE, setActiveLocale, SOURCE_LOCALE } from '@decryp7/paddock-ui/shell/i18n';
import {
  NONE,
  TransferMeter,
  formatBytes,
  formatCompact,
  formatCount,
  formatDateTime,
  formatDecimal,
  formatDuration,
  formatElapsed,
  formatEta,
  formatExactBytes,
  formatNumber,
  formatPercent,
  formatRate,
  formatRelativeTime,
  formatTime,
  percent,
  share,
} from './units';
import { DEFAULT_KEEP_TAIL, ELLIPSIS, fitEnd, fitPath, middleEllipsis, splitForEllipsis, splitPath } from './middle-ellipsis';

afterEach(() => setActiveLocale(SOURCE_LOCALE));

describe('bytes', () => {
  it('uses binary steps with Explorer labels and rounds up across a boundary', () => {
    expect([formatBytes(0), formatBytes(1), formatBytes(12), formatBytes(1023)]).toEqual(['0 bytes', '1 byte', '12 bytes', '1,023 bytes']);
    expect([formatBytes(1500), formatBytes(850 * 1024), formatBytes(1.2 * 1024 ** 3), formatBytes(1023.9 * 1024)]).toEqual(['1.5 KB', '850 KB', '1.2 GB', '1 MB']);
    expect(formatBytes(5 * 1024 ** 5)).toBe('5 PB');
  });
  it('shows a dash for anything that is not a size, and the exact value for a tooltip', () => {
    for (const bad of [null, undefined, -1, Number.NaN, Number.POSITIVE_INFINITY]) expect(formatBytes(bad)).toBe(NONE);
    expect(formatExactBytes(1_234_567)).toBe('1,234,567 bytes');
    expect(formatExactBytes(null)).toBe(NONE);
  });
  it('formats a rate and leaves it out while idle or unknown', () => {
    expect(formatRate(12.4 * 1024 * 1024)).toBe('12 MB/s');
    expect([formatRate(0), formatRate(null), formatRate(-5)]).toEqual(['', '', '']);
  });
});

describe('durations', () => {
  it('writes the two largest units people read', () => {
    expect([350, 4200, 42_000, 190_000, 7_500_000, 97_200_000].map(formatDuration)).toEqual(['350 ms', '4.2 s', '42 s', '3 min 10 s', '2 h 5 min', '1 d 3 h']);
    expect(formatDuration(-1)).toBe(NONE);
  });
  it('writes an elapsed clock', () => {
    expect([formatElapsed(0), formatElapsed(452_000), formatElapsed(4_052_000), formatElapsed(Number.NaN)]).toEqual(['00:00', '07:32', '1:07:32', NONE]);
  });
  it('rounds the time left to the unit people read and leaves it out while unknown', () => {
    expect([formatEta(20), formatEta(120), formatEta(7200), formatEta(7500), formatEta(3 * 86400)]).toEqual([
      'less than a minute left',
      'about 2 min left',
      'about 2 h left',
      'about 2 h 5 min left',
      'about 3 d left',
    ]);
    expect([formatEta(null), formatEta(-1)]).toEqual(['', '']);
  });
});

describe('counts and shares', () => {
  it('groups, shortens and keeps decimals', () => {
    expect([formatCount(12345), formatCount(Number.NaN)]).toEqual(['12,345', NONE]);
    expect([formatCompact(999), formatCompact(1234), formatCompact(25_000), formatCompact(2_500_000)]).toEqual(['999', '1.2k', '25k', '2.5M']);
    expect([formatNumber(2_500_000), formatNumber(25_000), formatNumber(250), formatNumber(25.43), formatNumber(2.5)]).toEqual(['2.5M', '25k', '250', '25.4', '2.5']);
    expect(formatDecimal(1.25, 1)).toBe('1.3');
    expect(formatDecimal(undefined)).toBe(NONE);
  });
  it('formats percentages and safe shares', () => {
    expect([formatPercent(42), formatPercent(42.5, 1), formatPercent(null)]).toEqual(['42%', '42.5%', NONE]);
    expect([percent(50, 200), percent(5, 0), percent(300, 200), percent(0, 10)]).toEqual([25, 0, 100, 0]);
    expect([share(1, 4), share(5, 0), share(9, 4), share(-1, 4)]).toEqual([0.25, 0, 1, 0]);
  });
  it('writes numbers in the active locale but keeps the pseudo-locale readable', () => {
    setActiveLocale('de-DE');
    expect(formatCount(12345)).toBe('12.345');
    expect(formatDecimal(1.5)).toBe('1,5');
    setActiveLocale(PSEUDO_LOCALE);
    expect(formatDecimal(1.5)).toBe('1.5');
  });
});

describe('times', () => {
  const now = Date.UTC(2026, 9, 24, 12, 0, 0);
  it('speaks of the past and the future, and says "just now" within two seconds', () => {
    expect(formatRelativeTime(now - 500, now)).toBe('just now');
    expect(formatRelativeTime(now + 1000, now)).toBe('just now');
    expect(formatRelativeTime(now - 12_000, now)).toBe('12 sec. ago');
    expect(formatRelativeTime(now - 5 * 60_000, now)).toBe('5 min. ago');
    expect(formatRelativeTime(now + 3 * 3_600_000, now)).toBe('in 3 hr.');
    expect(formatRelativeTime(now - 3 * 86_400_000, now)).toBe('3 days ago');
    expect(formatRelativeTime(null, now)).toBe(NONE);
    expect(formatRelativeTime(now, Number.NaN)).toBe(NONE);
  });
  it('formats clock times in the active locale', () => {
    const t = new Date(2026, 9, 24, 14, 5).getTime();
    expect(formatTime(t)).toMatch(/2:05|14:05/);
    expect(formatDateTime(t)).toMatch(/Oct/);
    expect([formatTime(null), formatDateTime(Number.NaN)]).toEqual([NONE, NONE]);
  });
});

describe('TransferMeter', () => {
  it('offers a rate and an ETA only after two seconds of samples, and restarts when counts go backwards', () => {
    const m = new TransferMeter();
    m.sample(0, 0);
    m.sample(1000, 1000);
    expect(m.rate).toBe(0);
    expect(m.etaSecs(10_000)).toBeNull();
    m.sample(2000, 2000);
    expect(m.rate).toBe(1000);
    expect(m.etaSecs(10_000)).toBe(8);
    m.sample(500, 3000);
    expect(m.rate).toBe(0);
    m.sample(Number.NaN, 4000);
    m.sample(500, 3000);
    m.reset();
    expect(m.etaSecs(1)).toBeNull();
  });
});

describe('middle ellipsis', () => {
  it('keeps the start and the file name of a path', () => {
    expect(middleEllipsis('C:\\ProgramData\\Builder\\Logs\\Builder.Service.log', 38)).toBe(`C:\\ProgramData\\${ELLIPSIS}\\Builder.Service.log`);
    expect(middleEllipsis('short', 10)).toBe('short');
    expect([middleEllipsis('abcdefghij', 0), middleEllipsis('abcdefghij', 1)]).toEqual(['', ELLIPSIS]);
    expect(middleEllipsis('abcdefghijklmnop', 8)).toBe(`abcd${ELLIPSIS}nop`);
  });
  it('splits for a layout ellipsis without breaking a surrogate pair', () => {
    expect(splitForEllipsis('/var/log/app/system.log')).toEqual({ head: '/var/log/app', tail: '/system.log' });
    expect(splitForEllipsis('short')).toEqual({ head: '', tail: 'short' });
    const long = `${'a'.repeat(30)}😀😀😀`;
    const { head, tail } = splitForEllipsis(long, 2);
    expect(tail).toBe('😀😀');
    expect(head + tail).toBe(long);
    expect(DEFAULT_KEEP_TAIL).toBe(20);
  });
  it('splits a path into directory, start of the name and tail, which shrink in that order (review 2, N-P2-4)', () => {
    expect(splitPath('C:\\ProgramData\\Builder\\Logs\\Builder.Service.2026-10-24.log')).toEqual({ dir: 'C:\\ProgramData\\Builder\\Logs\\', name: 'Builder.Service.', tail: '2026-10-24.log' });
    expect(splitPath('/var/log/app/system.log')).toEqual({ dir: '/var/log/app/', name: '', tail: 'system.log' });
    expect(splitPath('system.log')).toEqual({ dir: '', name: '', tail: 'system.log' });
  });
  it('starts a long name\'s tail at a word, never mid-word (review P2-10)', () => {
    const win = splitForEllipsis('C:\\ProgramData\\Builder\\Logs\\Builder.Service.2026-10-24.log');
    expect(win.tail).toBe('2026-10-24.log');
    expect(win.head + win.tail).toBe('C:\\ProgramData\\Builder\\Logs\\Builder.Service.2026-10-24.log');
    // No boundary in the kept characters: the last ones, as before.
    expect(splitForEllipsis(`${'a'.repeat(30)}${'b'.repeat(25)}`).tail).toBe('b'.repeat(20));
    // A boundary at the very end never empties the tail.
    expect(splitForEllipsis(`${'a'.repeat(30)}-`).tail.length).toBeGreaterThan(0);
  });
});

describe('fitPath (screenshot review P2-3, re-review P1)', () => {
  /** A 12 px mono face: every character is 7.2 px wide, so a box of 240 px holds 33 characters. */
  const mono = (s: string): number => Array.from(s).length * 7.2;
  const WIN = 'C:\\ProgramData\\Builder\\Logs\\Builder.Service.2026-10-24.log';
  const POSIX = '/Users/lin/Library/Application Support/Builder/logs/builder.service.log';
  const count = (s: string): number => s.split(ELLIPSIS).length - 1;

  it('returns a path that fits unchanged', () => {
    expect(fitPath(WIN, 1000, mono)).toBe(WIN);
  });
  it('collapses the folders into one ellipsis flush against the root and the separator first', () => {
    expect(fitPath(WIN, 35 * 7.2, mono)).toBe('C:\\…\\Builder.Service.2026-10-24.log');
    expect(fitPath(POSIX, 240, mono)).toBe('/Users/lin/…/builder.service.log');
    expect(fitPath(POSIX, 28 * 7.2, mono)).toBe('/Users/…/builder.service.log');
    expect(fitPath(POSIX, 22 * 7.2, mono)).toBe('/…/builder.service.log');
    expect(fitPath('\\\\srv\\share\\dir\\sub\\system.log', 24 * 7.2, mono)).toBe('\\\\srv\\share\\…\\system.log');
    expect(fitPath('logs/app/deep/system.log', 15 * 7.2, mono)).toBe('log…/system.log');
  });
  it('never drops the root: when the name does not fit either, the ellipsis stays against the root and the name loses its start (Windows, 240 and 220 px)', () => {
    expect(fitPath(WIN, 240, mono)).toBe('C:\\…Service.2026-10-24.log');
    expect(fitPath(WIN, 220, mono)).toBe('C:\\…Service.2026-10-24.log');
    for (const width of [240, 220, 150, 60, 30]) {
      const shown = fitPath(WIN, width, mono);
      expect(shown.startsWith('C:\\' + ELLIPSIS), `${width}: ${shown}`).toBe(true);
      expect(count(shown)).toBe(1);
      expect(mono(shown)).toBeLessThanOrEqual(Math.max(width, 4 * 7.2));
    }
  });
  it('does the same for POSIX and UNC paths, keeping the extension', () => {
    expect(fitPath(POSIX, 150, mono)).toBe('/…service.log');
    expect(fitPath(POSIX, 12 * 7.2, mono)).toBe('/…log');
    expect(fitPath('\\\\srv\\share\\dir\\sub\\system.log', 20 * 7.2, mono)).toBe('\\\\srv\\share\\…log');
    expect(fitPath('/x.log', 3 * 7.2, mono)).toBe('/…g');
  });
  it('the tail never starts mid-word: the name is cut after a . - _ or space, or at a lower-to-upper change, and only a name with no boundary that fits is cut at a character (re-review P2)', () => {
    const boundary = /^[^]*…(?:[A-Z]|(?<=[.\-_ ])|(?<=[a-z])(?=[A-Z]))/;
    for (const path of [WIN, POSIX, 'C:\\Logs\\BuilderServiceHost2026.log', '/var/log/my service name.log']) {
      const name = path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);
      for (let width = 60; width < 400; width += 7) {
        const shown = fitPath(path, width, mono);
        if (!shown.includes(ELLIPSIS) || shown.endsWith(ELLIPSIS) || shown.endsWith(`${ELLIPSIS}\\${name}`) || shown.endsWith(`${ELLIPSIS}/${name}`)) continue;
        const tail = shown.slice(shown.indexOf(ELLIPSIS) + 1);
        const at = name.length - tail.length;
        if (at <= 0 || !name.endsWith(tail)) continue; // a folders-only cut
        const mid = !/[.\-_ ]/.test(name[at - 1] ?? '') && !(/[a-z]/.test(name[at - 1] ?? '') && /[A-Z]/.test(name[at] ?? ''));
        // Mid-word only when no word start of the name would have fitted.
        const fitting = Array.from(name).some((_, i) => i > 0 && i < at && (/[.\-_ ]/.test(name[i - 1] ?? '') || (/[a-z]/.test(name[i - 1] ?? '') && /[A-Z]/.test(name[i] ?? ''))) && mono(shown.slice(0, shown.indexOf(ELLIPSIS) + 1) + name.slice(i)) <= width);
        expect(mid && fitting, `${width}: ${shown}`).toBe(false);
      }
    }
    expect(boundary.test('C:\\…Service.log')).toBe(true);
    expect(fitPath('C:\\Logs\\BuilderServiceHost2026.log', 17 * 7.2, mono)).toBe('C:\\…Host2026.log');
    expect(fitPath('/var/log/my service name.log', 16 * 7.2, mono)).toBe('/…name.log');
  });
  it('gives the root and the ellipsis alone when nothing of the name fits', () => {
    expect(fitPath(WIN, 20, mono)).toBe('C:\\…');
  });
  it('shortens a bare name in front of its last words, with one ellipsis and no gap', () => {
    expect(fitPath('a-rather-long-service-name-2026-10-24-11-30.log', 150, mono)).toBe('…026-10-24-11-30.log');
    expect(fitPath('short-name.log', 50, mono)).toBe('…e.log');
    expect(fitPath(POSIX, 0, mono)).toBe('/…');
  });
});

describe('fitEnd (screenshot review P3)', () => {
  const m = (s: string): number => Array.from(s).length * 8;
  it('returns text that fits and cuts the end with no space before the ellipsis', () => {
    expect(fitEnd('Paddock Shell Demo', 400, m)).toBe('Paddock Shell Demo');
    expect(fitEnd('Paddock Shell Demo', 9 * 8, m)).toBe('Paddock…');
    expect(fitEnd('Paddock Shell Demo', 10 * 8, m)).toBe('Paddock S…');
    expect(fitEnd('Paddock Shell Demo', 0, m)).toBe('…');
  });
});
