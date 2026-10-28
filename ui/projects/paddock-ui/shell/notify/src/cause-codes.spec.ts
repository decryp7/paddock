import { KNOWN_CAUSE_CODES } from './service-status-text';

// ER-21: every health cause code the Rust side emits (`Cause::code(...)`) has words in the chip's table, and every word in the
// table is for a code Rust emits. A `.code(` whose argument is not a string literal must be listed in NON_LITERAL with the codes it
// can produce, so a new emitter cannot hide from the scan. Read from disk; the repo root is found by walking up to `crates/`.
// Follow-up (docs/plans/shared-next.md): declare the codes once in Rust and generate the TypeScript union.

interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  readdirSync(path: string): string[];
  statSync(path: string): { isDirectory(): boolean };
  existsSync(path: string): boolean;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;

/** Non-literal `.code(<argument>)` calls: `<path suffix>|<argument text>` to the codes the call can produce. */
const NON_LITERAL: Readonly<Record<string, readonly string[]>> = {
  // The `down(code, ...)` helper: its codes are the literals of the `down(` calls, which the scan below reads.
  'paddock-service/src/status.rs|format!("service.{code}")': [],
  'paddock-service/src/status.rs|code': ['service.switchedOff', 'service.needsApproval'],
  'paddock-pitwall/src/health.rs|code': ['pitwall.notInstalled', 'pitwall.notRunning', 'pitwall.incompatible', 'pitwall.needsPairing', 'pitwall.paused', 'pitwall.noModel', 'pitwall.unavailable'],
  // Passes a notice's own code on: the codes are the callers' (their own `.code(` calls).
  'paddock-health/src/notify/health.rs|code.clone()': [],
};

/** Table entries no non-test Rust code emits, each with the reason it stays. */
const TABLE_ONLY: Readonly<Record<string, string>> = {};

function rustSources(fs: NodeFs, dir: string): string[] {
  return fs.readdirSync(dir).flatMap((name) => {
    if (name === 'target' || name === 'node_modules' || name === 'tests' || name === 'benches') return [];
    const path = `${dir}/${name}`;
    if (fs.statSync(path).isDirectory()) return rustSources(fs, path);
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
    for (const file of [...rustSources(fs, `${root}/crates`), ...rustSources(fs, `${root}/apps`)]) {
      const code = withoutTestModules(fs.readFileSync(file, 'utf8'));
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
