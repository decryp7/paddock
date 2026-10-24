import { SHELL_PLUGIN, SHELL_VERSION } from '../generated/shell';
import { SHELL_COMMANDS } from '../generated/shell-commands';
import { checkShellVersion, isShellCommand, shellCommand, shellInvoke, shellVersionProblem } from './plugin';

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
const nodeFs = (): Promise<{ readFileSync(path: string, encoding: 'utf8'): string }> =>
  import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<{ readFileSync(path: string, encoding: 'utf8'): string }>;

describe('the shell plugin', () => {
  it('sends the shell’s commands to the plugin and the rest as they are', async () => {
    const sent: string[] = [];
    const invoke = shellInvoke(<T>(command: string) => (sent.push(command), Promise.resolve(null as T)));
    await invoke('health_snapshot');
    await invoke('demo_status');
    await invoke('plugin:event|listen');
    expect(sent).toEqual([`plugin:${SHELL_PLUGIN}|health_snapshot`, 'demo_status', 'plugin:event|listen']);
    expect([isShellCommand('notifications_post'), isShellCommand('toString'), isShellCommand('demo_status')]).toEqual([true, false, false]);
    expect(shellCommand('shell_version')).toBe('plugin:paddock-shell|shell_version');
    expect(Object.keys(SHELL_COMMANDS).length).toBeGreaterThan(40);
  });

  it('carries the package’s version (read from package.json, rule 6)', async () => {
    const fs = await nodeFs();
    const pkg = JSON.parse(fs.readFileSync('projects/paddock-ui/package.json', 'utf8')) as { version: string };
    expect(pkg.version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(SHELL_VERSION).toBe(pkg.version);
  });

  it('says plainly when the plugin and the package disagree', async () => {
    expect(shellVersionProblem({ shell: SHELL_VERSION, app: { id: 'a', name: 'A', version: '1.0.0' } })).toBeNull();
    expect(shellVersionProblem({ shell: '0.0.1' }, '0.0.2')).toBe('the paddock-shell plugin is 0.0.1 but @decryp7/paddock-ui is 0.0.2: bump both to the same version');
    expect(shellVersionProblem(null, '0.0.2')).toContain('did not say its version');
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const asked: string[] = [];
    const mismatch = await checkShellVersion(<T>(c: string) => (asked.push(c), Promise.resolve({ shell: '9.9.9' } as T)), '0.2.0');
    expect(asked).toEqual(['plugin:paddock-shell|shell_version']);
    expect(mismatch).toContain('9.9.9');
    expect(error).toHaveBeenCalledWith(expect.stringContaining('bump both'));
    const missing = await checkShellVersion(() => Promise.reject(new Error('no such plugin')), '0.2.0');
    expect(missing).toContain('did not say');
    expect(error).toHaveBeenCalledWith(expect.stringContaining('no such plugin'));
    await expect(checkShellVersion(<T>() => Promise.resolve({ shell: '0.2.0' } as T), '0.2.0')).resolves.toBeNull();
    error.mockRestore();
  });
});
