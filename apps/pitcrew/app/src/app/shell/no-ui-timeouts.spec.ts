import { uiTimeoutProblems } from '@decryp7/paddock-ui/shell/jobs';

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readdirSync(path: string, options: { withFileTypes: true }): { name: string; isDirectory(): boolean }[];
  readFileSync(path: string, encoding: 'utf8'): string;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;

function sources(fs: NodeFs, dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = `${dir}/${e.name}`;
    if (e.isDirectory()) return sources(fs, path);
    return e.name.endsWith('.ts') && !e.name.endsWith('.spec.ts') ? [path] : [];
  });
}

// Rule 12: the UI never gives up on running work. The shell's check runs over every source file of the app.
describe('the UI has no timeouts on running work (rule 12)', () => {
  it('has no timer that abandons or fails an awaited operation', async () => {
    const fs = await nodeFs();
    const files = sources(fs, 'src/app');
    expect(files.length).toBeGreaterThan(50);
    const problems = files.flatMap((f) => uiTimeoutProblems(f, fs.readFileSync(f, 'utf8')));
    expect(problems).toEqual([]);
  });
});
