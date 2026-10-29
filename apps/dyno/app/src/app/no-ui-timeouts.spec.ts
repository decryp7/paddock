import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { uiTimeoutProblems } from '@decryp7/paddock-ui/shell/jobs';

/** Every source file of the app (not its specs): the UI never gives up on running work (ER-19, ADR 0008 §4). */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.ts$/.test(name) && !/\.spec\.ts$/.test(name) && !/\.d\.ts$/.test(name) ? [path] : [];
  });
}

describe('no UI timeouts', () => {
  it('no source file times out, abandons or fails running work', () => {
    const root = join(process.cwd(), 'src');
    const files = sources(root);
    expect(files.length).toBeGreaterThan(100);
    const problems = files.flatMap((f) => uiTimeoutProblems(f.slice(root.length + 1), readFileSync(f, 'utf8')));
    expect(problems).toEqual([]);
  });
});
