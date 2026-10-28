import { KNOWN_CAUSE_CODES } from './service-status-text';

// ER-21: every health cause code the Rust side emits (`Cause::code("…")`, and the `service.<x>` codes of paddock-service) has
// words in the chip's table, so a new code cannot ship without someone deciding what the chip says. Read from disk (vitest
// runs in ui/); test-only files are skipped.

interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  readdirSync(path: string): string[];
  statSync(path: string): { isDirectory(): boolean };
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;

function rustSources(fs: NodeFs, dir: string): string[] {
  return fs.readdirSync(dir).flatMap((name) => {
    if (name === 'target' || name === 'node_modules' || name === 'tests' || name === 'benches') return [];
    const path = `${dir}/${name}`;
    if (fs.statSync(path).isDirectory()) return rustSources(fs, path);
    return path.endsWith('.rs') && name !== 'tests.rs' ? [path] : [];
  });
}

describe('health cause codes', () => {
  it('every code Rust emits has words in the chip table', async () => {
    const fs = await nodeFs();
    const emitted = new Map<string, string>();
    for (const file of [...rustSources(fs, '../crates'), ...rustSources(fs, '../apps')]) {
      const text = fs.readFileSync(file, 'utf8');
      // Drop the unit tests inside the file.
      const code = text.split('#[cfg(test)]')[0] ?? '';
      for (const m of code.matchAll(/\.code\("([A-Za-z0-9_.-]+)"\)/g)) emitted.set(m[1] ?? '', file);
      if (file.endsWith('paddock-service/src/status.rs')) {
        for (const m of code.matchAll(/^\s*"([a-zA-Z]+)",$/gm)) emitted.set(`service.${m[1]}`, file);
        for (const m of code.matchAll(/"(service\.[a-zA-Z]+)"/g)) emitted.set(m[1] ?? '', file);
      }
    }
    expect(emitted.size).toBeGreaterThan(10);
    const missing = [...emitted].filter(([c]) => !KNOWN_CAUSE_CODES.includes(c)).map(([c, f]) => `${c} (${f})`);
    expect(missing).toEqual([]);
  });
});
