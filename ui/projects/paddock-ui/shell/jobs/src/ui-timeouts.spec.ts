import { stripCommentsAndStrings, uiTimeoutProblems } from './ui-timeouts';

describe('the no-UI-timeout check', () => {
  it.each([
    ['a delayed setTimeout', 'setTimeout(() => fail(), 30_000);', 'a delayed setTimeout'],
    ['a setTimeout without a delay argument at 0', 'setTimeout(stop, ms);', 'a delayed setTimeout'],
    ['AbortSignal.timeout', 'fetch(url, { signal: AbortSignal.timeout(5000) });', 'timeout(...) times out work'],
    ['an rxjs timeout', 'source.pipe(timeout(10_000));', 'timeout(...) times out work'],
    ['a call-timeout helper', 'await callTimeout(invoke("x"), 30);', 'callTimeout(...) times out work'],
    ['a generic timeout helper', 'withTimeout<number>(p, 5)', 'withTimeout(...) times out work'],
    ['Promise.race', 'await Promise.race([job, timer]);', 'Promise.race'],
  ])('flags %s', (_what, code, problem) => {
    const found = uiTimeoutProblems('a.ts', code);
    expect(found).toHaveLength(1);
    expect(found[0]).toContain(problem);
    expect(found[0]).toMatch(/^a\.ts:1: /);
  });

  it('allows a zero-delay macrotask, clearTimeout, and words in comments and strings', () => {
    const code = [
      'else setTimeout(work, 0);',
      'clearTimeout(handle);',
      '// setTimeout(() => fail(), 30_000)',
      '/* Promise.race([a, b]) */',
      "const s = 'setTimeout(x, 5)';",
      'const t = "callTimeout(1)";',
      'const u = `timeout(${1})`;',
      "const e = 'it\\'s timeout(2)';",
      'requestIdleCallback(work, { timeout: 2000 });',
      'export function callTimeoutProblems(path: string) {}',
    ].join('\n');
    expect(uiTimeoutProblems('b.ts', code)).toEqual([]);
  });

  it('reports the line of each problem', () => {
    expect(uiTimeoutProblems('c.ts', 'a();\n/* x\ny */\nsetTimeout(f, 10);')).toEqual(['c.ts:4: a delayed setTimeout (the UI never times out work)']);
  });

  it('keeps line breaks and blanks unterminated comments and strings', () => {
    expect(stripCommentsAndStrings('a // b\nc')).toBe('a     \nc');
    expect(stripCommentsAndStrings('a /* b')).toBe('a     ');
    expect(stripCommentsAndStrings("x = 'ab")).toBe("x = '  ");
    expect(stripCommentsAndStrings('x // end')).toBe('x       ');
  });
});
