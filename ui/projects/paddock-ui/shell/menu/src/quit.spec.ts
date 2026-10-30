import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ContributionRegistry, provideContributions, provideShellContext, provideShellCore, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { QUIT_COMMAND } from '../generated/menu';
import { provideShellQuit, QUIT_CONTRIBUTIONS } from './quit';

describe('Quit', () => {
  it('is one command with Mod+Q, last in the app menu', () => {
    expect(QUIT_COMMAND).toBe('pd.app.quit');
    expect(QUIT_CONTRIBUTIONS.commands?.map((c) => c.id)).toEqual([QUIT_COMMAND]);
    expect(QUIT_CONTRIBUTIONS.keybindings?.map((k) => [k.command, k.key])).toEqual([[QUIT_COMMAND, 'Mod+Q']]);
    const item = QUIT_CONTRIBUTIONS.menus?.[0];
    if (!item) throw new Error('no menu item');
    expect([item.menu, item.command, item.group]).toEqual(['app', QUIT_COMMAND, 'quit']);
    expect(item.order).toBeGreaterThan(100);
  });

  it('asks the shell plugin to quit', async () => {
    const calls: string[] = [];
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideShellCore(),
        provideContributions(QUIT_CONTRIBUTIONS),
        provideShellQuit(<T>(command: string) => {
          calls.push(command);
          return Promise.resolve(undefined as T);
        }),
      ],
    });
    TestBed.inject(ContributionRegistry);
    const ctx = await TestBed.inject(ShellContextHost).load();
    await ctx.quitApp();
    expect(calls).toEqual(['plugin:paddock-shell|app_quit']);
    expect(await ctx.runCommand(QUIT_COMMAND)).toBe(true);
    expect(calls).toHaveLength(2);
  });

  it('only warns in a window without a quit', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellCore(), provideShellContext(() => Promise.resolve(() => ({})))] });
    const ctx = await TestBed.inject(ShellContextHost).load();
    await ctx.quitApp();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('provideShellQuit'));
    warn.mockRestore();
  });
});
