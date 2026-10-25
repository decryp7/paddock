import { contrast, deltaE, parseHex, simulate, type Deficiency } from './color';
import { DARK, LIGHT, type ThemeTokens } from './tokens';

/** Text pairs the UI uses (foreground on background) that must reach WCAG AA for normal text (4.5:1). */
function textPairs(t: ThemeTokens): [string, string, string][] {
  const surfaces: [string, string][] = [
    ['surface2', t.surface2],
    ['surface3', t.surface3],
    ['inset', t.inset],
    ['overlay', t.overlay],
    ['bg', t.bg],
  ];
  const pairs: [string, string, string][] = [];
  for (const [name, bg] of surfaces) {
    for (const fg of ['text1', 'text2', 'text3', 'accentText', 'ok', 'warn', 'bad', 'info'] as const) pairs.push([`${fg} on ${name}`, t[fg], bg]);
  }
  pairs.push(['textOnAccent on accent', t.textOnAccent, t.accent]);
  pairs.push(['accentText on accentSoft', t.accentText, t.accentSoft]);
  pairs.push(['ok on okSoft', t.ok, t.okSoft]);
  pairs.push(['warn on warnSoft', t.warn, t.warnSoft]);
  pairs.push(['bad on badSoft', t.bad, t.badSoft]);
  pairs.push(['info on infoSoft', t.info, t.infoSoft]);
  return pairs;
}

describe.each([
  ['light', LIGHT],
  ['dark', DARK],
])('%s theme tokens', (_name, t) => {
  it.each(textPairs(t))('%s reaches 4.5:1', (_label, fg, bg) => {
    expect(contrast(fg, bg)).toBeGreaterThanOrEqual(4.5);
  });

  it('UI marks (accent, focus ring, status colours) reach 3:1 on the card', () => {
    for (const c of [t.accent, t.focus, t.ok, t.warn, t.bad, t.info]) expect(contrast(c, t.surface2)).toBeGreaterThanOrEqual(3);
  });

  it('chart series reach 3:1 against the card', () => {
    for (const c of t.chart) expect(contrast(c, t.surface2)).toBeGreaterThanOrEqual(3);
  });

  it.each<Deficiency | 'normal'>(['normal', 'protanopia', 'deuteranopia', 'tritanopia'])('the first five chart colours stay apart with %s vision', (kind) => {
    const series = [...t.chart.slice(0, 5)];
    const seen = series.map((c) => (kind === 'normal' ? parseHex(c) : simulate(c, kind)));
    let min = Infinity;
    for (let i = 0; i < seen.length; i++) for (let j = i + 1; j < seen.length; j++) min = Math.min(min, deltaE(seen[i]!, seen[j]!));
    expect(min).toBeGreaterThanOrEqual(kind === 'normal' ? 20 : 10);
  });

  it('the chart colours stay apart from the brand accent', () => {
    for (const c of t.chart.slice(0, 5)) expect(deltaE(parseHex(c), parseHex(t.accent))).toBeGreaterThanOrEqual(15);
  });
});
