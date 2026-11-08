import { readBounded, rustSources, scanEmitted, withoutTestModules } from './cause-codes-scan';

// The helpers and the scan are in cause-codes-scan.ts; the check over the real tree is cause-codes.scan.test.mjs (node --test).
// Only pure logic is judged here, so the result never depends on the disk or the machine's load.

describe('health cause codes', () => {
  it('lists Rust sources without a stat per entry and without entering skipped folders', () => {
    const tree: Record<string, { name: string; dir: boolean }[]> = {
      r: [{ name: 'a.rs', dir: false }, { name: 'tests', dir: true }, { name: 'target', dir: true }, { name: 'src', dir: true }, { name: 'x.ts', dir: false }],
      'r/src': [{ name: 'b.rs', dir: false }, { name: 'tests.rs', dir: false }, { name: 'sub', dir: true }],
      'r/src/sub': [{ name: 'c.rs', dir: false }],
    };
    const read: string[] = [];
    const fake = {
      readdirSync: (dir: string, o: { withFileTypes: true }) => {
        expect(o.withFileTypes).toBe(true);
        read.push(dir);
        return (tree[dir] ?? []).map((e) => ({ name: e.name, isDirectory: () => e.dir }));
      },
    };
    expect(rustSources(fake, 'r')).toEqual(['r/a.rs', 'r/src/b.rs', 'r/src/sub/c.rs']);
    expect(read).toEqual(['r', 'r/src', 'r/src/sub']);
  });

  it('reads files with a bounded number in flight and keeps the order', async () => {
    let flying = 0;
    let peak = 0;
    const files = Array.from({ length: 20 }, (_, i) => `f${i}`);
    const out = await readBounded(
      files,
      async (f) => {
        peak = Math.max(peak, ++flying);
        await Promise.resolve();
        await Promise.resolve();
        flying--;
        return `text ${f}`;
      },
      4,
    );
    expect(out).toEqual(files.map((f) => `text ${f}`));
    expect(peak).toBe(4);
    expect(await readBounded([], async () => 'x', 4)).toEqual([]);
  });

  it('strips only test modules', () => {
    const src = 'a(); #[cfg(test)] mod t { fn x() { b(); } } c();';
    expect(withoutTestModules(src)).toBe('a();  c();');
  });

  it('finds the emitted codes, lists a non-literal call it cannot read, and skips test modules', () => {
    const files = ['c/a/src/x.rs', 'c/paddock-pitwall/src/health.rs', 'c/b/src/y.rs', 'c/z.rs'];
    const texts = [
      'Cause::new().code("a.one"); #[cfg(test)] mod t { fn f() { c.code("a.test"); } }',
      'c.code(code)',
      'c.code(format!("{x}"))',
      'no calls here',
    ];
    const { emitted, unlisted } = scanEmitted(files, texts);
    expect([...emitted.keys()].sort()).toEqual(['a.one', 'pitwall.incompatible', 'pitwall.needsPairing', 'pitwall.noModel', 'pitwall.notInstalled', 'pitwall.notRunning', 'pitwall.paused', 'pitwall.unavailable']);
    expect(emitted.get('a.one')).toBe('c/a/src/x.rs');
    expect(unlisted).toEqual(['c/b/src/y.rs: .code(format!("{x}"))']);
  });
});
