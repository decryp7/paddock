import { KNOWN_CAUSE_CODES } from './service-status-text';

// ER-21: every health cause code the Rust side emits (`Cause::code(...)`) has words in the chip's table, and every word in the
// table is for a code Rust emits. A `.code(` whose argument is not a string literal must be listed in NON_LITERAL with the codes it
// can produce, so a new emitter cannot hide from the scan. Read from disk; the repo root is found by walking up to `crates/`.
// Follow-up (docs/plans/shared-next.md): declare the codes once in Rust and generate the TypeScript union.

/** What the source listing needs: one directory read that already says which entries are folders (no stat per entry). */
interface ListFs {
  readdirSync(path: string, options: { withFileTypes: true }): { name: string; isDirectory(): boolean }[];
}
interface NodeFs extends ListFs {
  promises: { readFile(path: string, encoding: 'utf8'): Promise<string> };
  existsSync(path: string): boolean;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;

/** Sources read at once: SSD I/O wants a few requests in flight (ER-20); more only queues in the kernel. */
export const READ_CONCURRENCY = 8;

/** Reads every file with at most `limit` reads in flight, results in input order. A cold volume answers many small reads in
 * parallel far faster than one after another (2.2k files, 24 MB: ~5 s serial from a cold cache). */
export async function readBounded(files: readonly string[], read: (file: string) => Promise<string>, limit: number): Promise<string[]> {
  const out = new Array<string>(files.length);
  let next = 0;
  const worker = async (): Promise<void> => {
    for (let i = next++; i < files.length; i = next++) out[i] = await read(files[i] ?? '');
  };
  await Promise.all(Array.from({ length: Math.min(limit, files.length) }, worker));
  return out;
}

/** Non-literal `.code(<argument>)` calls: `<path suffix>|<argument text>` to the codes the call can produce. */
const NON_LITERAL: Readonly<Record<string, readonly string[]>> = {
  // The `down(code, ...)` helper: its codes are the literals of the `down(` calls, which the scan below reads.
  'paddock-service/src/status.rs|format!("service.{code}")': [],
  'paddock-service/src/status.rs|code': ['service.switchedOff', 'service.needsApproval'],
  'paddock-pitwall/src/health.rs|code': ['pitwall.notInstalled', 'pitwall.notRunning', 'pitwall.incompatible', 'pitwall.needsPairing', 'pitwall.paused', 'pitwall.noModel', 'pitwall.unavailable'],
  // `Cause::from_limit`: the code is `limit.<id>` for any LimitId. Its words are the limit's own (LimitNotice, plan 2.3, one message per
  // LimitId), not a chip-table entry, so no table words are required for it.
  'paddock-health/src/limit_cause.rs|format!("limit.{}", limit.id)': [],
  // Passes a notice's own code on: the codes are the callers' (their own `.code(` calls).
  'paddock-health/src/notify/health.rs|code.clone()': [],
};

/** Table entries no non-test Rust code emits, each with the reason it stays. */
const TABLE_ONLY: Readonly<Record<string, string>> = {};

export function rustSources(fs: ListFs, dir: string): string[] {
  // `withFileTypes` gives the entry kind from the directory read itself: a `statSync` per entry is a disk round trip each, which on a
  // cold volume (a fresh worktree) took 4 s to 90 s over the ~2k Rust sources' folders and timed the spec out.
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const name = entry.name;
    if (name === 'target' || name === 'node_modules' || name === 'tests' || name === 'benches') return [];
    const path = `${dir}/${name}`;
    if (entry.isDirectory()) return rustSources(fs, path);
    return path.endsWith('.rs') && name !== 'tests.rs' ? [path] : [];
  });
}

/** The source without its `#[cfg(test)] mod x { ... }` blocks (braces are counted, strings are not parsed: good enough for a scan). */
export function withoutTestModules(text: string): string {
  let out = '';
  let rest = text;
  for (;;) {
    const at = rest.search(/#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+\w+\s*\{/);
    if (at < 0) return out + rest;
    out += rest.slice(0, at);
    let i = rest.indexOf('{', at);
    let depth = 0;
    for (; i < rest.length; i++) {
      if (rest[i] === '{') depth++;
      else if (rest[i] === '}' && --depth === 0) break;
    }
    rest = rest.slice(i + 1);
  }
}

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

  it('the Rust sources and the chip table agree', async () => {
    const fs = await nodeFs();
    const root = ['.', '..', '../..', '../../..'].find((p) => fs.existsSync(`${p}/crates/Cargo.toml`));
    expect(root, 'the repo root (a folder with crates/Cargo.toml) above the working folder').toBeDefined();
    const emitted = new Map<string, string>();
    const unlisted: string[] = [];
    const files = [...rustSources(fs, `${root}/crates`), ...rustSources(fs, `${root}/apps`)];
    const texts = await readBounded(files, (f) => fs.promises.readFile(f, 'utf8'), READ_CONCURRENCY);
    for (const [at, file] of files.entries()) {
      const text = texts[at] ?? '';
      // Most of the ~2k files never call `.code(`: skip the scan for them (same result, far less regex work).
      if (!text.includes('.code(')) continue;
      const code = withoutTestModules(text);
      for (const m of code.matchAll(/\.code\("([A-Za-z0-9_.-]+)"\)/g)) emitted.set(m[1] ?? '', file);
      for (const m of code.matchAll(/\.code\(((?:[^()"]|\([^()]*\)|"[^"]*")+)\)/g)) {
        const arg = (m[1] ?? '').trim();
        if (/^"[A-Za-z0-9_.-]+"$/.test(arg)) continue;
        const key = Object.keys(NON_LITERAL).find((k) => file.endsWith(k.split('|')[0] ?? '') && k.split('|')[1] === arg);
        if (key === undefined) unlisted.push(`${file}: .code(${arg})`);
        else for (const c of NON_LITERAL[key] ?? []) emitted.set(c, file);
      }
      if (file.endsWith('paddock-service/src/status.rs')) for (const m of code.matchAll(/^\s*"([a-zA-Z]+)",$/gm)) emitted.set(`service.${m[1]}`, file);
    }
    expect(unlisted, 'a .code( call with a non-literal argument: list it in NON_LITERAL with the codes it can produce').toEqual([]);
    expect(emitted.size).toBeGreaterThan(10);
    expect([...emitted].filter(([c]) => !KNOWN_CAUSE_CODES.includes(c)).map(([c, f]) => `${c} (${f})`)).toEqual([]);
    expect(KNOWN_CAUSE_CODES.filter((c) => !emitted.has(c) && !(c in TABLE_ONLY))).toEqual([]);
  });
});
