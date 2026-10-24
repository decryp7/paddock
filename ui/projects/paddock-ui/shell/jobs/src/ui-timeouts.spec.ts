import { callArguments, stripCommentsAndStrings, uiTimeoutProblems } from './ui-timeouts';

describe('the no-UI-timeout check (rule 12, narrowed by plan §12)', () => {
  it.each([
    ['a timer that rejects the request', 'new Promise((resolve, reject) => { setTimeout(() => reject(new Error("slow")), 30_000); });', 'a setTimeout that gives up on work'],
    ['a timer that is the rejection', 'setTimeout(reject, ms);', 'a setTimeout that gives up on work'],
    ['a timer that aborts a fetch', 'const t = setTimeout(() => controller.abort(), 5000);', 'a setTimeout that gives up on work'],
    ['a timer that cancels a job', 'setTimeout(() => void jobs.cancel(id), LIMIT_MS);', 'a setTimeout that gives up on work'],
    ['a timer that marks the work failed', 'setTimeout(() => this.failed.set(true), 10_000);', 'a setTimeout that gives up on work'],
    ['a timer that throws', 'setTimeout(() => { throw new Error("late"); }, 100);', 'a setTimeout that gives up on work'],
    ['an interval that gives up', 'setInterval(() => { if (late()) giveUp(); }, 1000);', 'a setInterval that gives up on work'],
    ['AbortSignal.timeout', 'fetch(url, { signal: AbortSignal.timeout(5000) });', 'timeout(...) times out work'],
    ['an rxjs timeout', 'source.pipe(timeout(10_000));', 'timeout(...) times out work'],
    ['a call-timeout helper', 'await callTimeout(invoke("x"), 30);', 'callTimeout(...) times out work'],
    ['a generic timeout helper', 'withTimeout<number>(p, 5)', 'withTimeout(...) times out work'],
    ['Promise.race with a sleep', 'await Promise.race([job, new Promise((r) => setTimeout(r, 5000))]);', 'Promise.race against a timer'],
    ['Promise.race with a named timer', 'await Promise.race([job, timer]);', 'Promise.race against a timer'],
    ['Promise.race with a deadline', 'await Promise.race([job, deadline(5)]);', 'Promise.race against a timer'],
  ])('flags %s', (_what, code, problem) => {
    const found = uiTimeoutProblems('a.ts', code);
    expect(found).toHaveLength(1);
    expect(found[0]).toContain(problem);
    expect(found[0]).toMatch(/^a\.ts:1: /);
  });

  it.each([
    ['a debounce', 'clearTimeout(timer);\ntimer = setTimeout(fit, REFIT_DEBOUNCE_MS);'],
    ['a retry of a read', 'next = setTimeout(attempt, options.timeoutMs);'],
    ['a sleep between polls', 'await new Promise<void>((r) => setTimeout(r, 250));'],
    ['a toast that hides itself', 'setTimeout(() => this.dismissToast(cause), 8000);'],
    ['a tick', 'const handle = setInterval(tick, ms);\nclearInterval(handle);'],
    ['a zero-delay macrotask', 'else setTimeout(work, 0);'],
    ['a race of two answers', 'const first = await Promise.race([fromCache, fromEngine]);'],
  ])('allows %s', (_what, code) => {
    expect(uiTimeoutProblems('b.ts', code)).toEqual([]);
  });

  it('ignores words in comments and strings', () => {
    const code = [
      '// setTimeout(() => reject(e), 30_000)',
      '/* Promise.race([a, timer]) */',
      "const s = 'setTimeout(() => abort(), 5)';",
      'const t = "callTimeout(1)";',
      'const u = `timeout(${1})`;',
      "const e = 'it\\'s timeout(2)';",
      'requestIdleCallback(work, { timeout: 2000 });',
      'export function callTimeoutProblems(path: string) {}',
      "setTimeout(() => log('cancelled by the user'), 10);",
    ].join('\n');
    expect(uiTimeoutProblems('b.ts', code)).toEqual([]);
  });

  it('reports the line of each problem', () => {
    expect(uiTimeoutProblems('c.ts', 'a();\n/* x\ny */\nsetTimeout(() => reject(), 10);')).toEqual(['c.ts:4: a setTimeout that gives up on work (the UI never times out work; show "Still working")']);
  });

  it('reads a call’s arguments to the matching parenthesis', () => {
    expect(callArguments('f(a, g(b), c) + d', 1)).toBe('a, g(b), c');
    expect(callArguments('f(a, (b', 1)).toBe('a, (b');
  });

  it('keeps line breaks and blanks unterminated comments and strings', () => {
    expect(stripCommentsAndStrings('a // b\nc')).toBe('a     \nc');
    expect(stripCommentsAndStrings('a /* b')).toBe('a     ');
    expect(stripCommentsAndStrings("x = 'ab")).toBe("x = '  ");
    expect(stripCommentsAndStrings('x // end')).toBe('x       ');
  });
});
