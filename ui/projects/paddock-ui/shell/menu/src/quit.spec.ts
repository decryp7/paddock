import { provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { AppMenu, MENU_MODE, MENU_OS, provideMenus, QUIT_TITLE } from './app-menu';
import { APP_QUIT, provideContributions, ContributionRegistry, provideShellContext, provideShellCore, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { QUIT_COMMAND } from '../generated/menu';
import { currentOs, currentPlatform } from './keys';
import { provideNativeMenu, tauriNativeMenuPort } from './native-menu';
import { QUIT_CONTRIBUTIONS } from './quit';

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

  it('is registered by the native menu provider, with the port as the default quit', async () => {
    const calls: string[] = [];
    const port = tauriNativeMenuPort({
      invoke: (command) => {
        calls.push(command);
        return Promise.resolve(undefined);
      },
      listen: () => Promise.resolve(() => undefined),
    });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellCore(), provideNativeMenu({ port })] });
    expect(TestBed.inject(ContributionRegistry).commands().map((c) => c.id)).toContain(QUIT_COMMAND);
    const ctx = await TestBed.inject(ShellContextHost).load();
    await ctx.quitApp();
    expect(calls).toEqual(['plugin:paddock-shell|app_quit']);
    expect(await ctx.runCommand(QUIT_COMMAND)).toBe(true);
    expect(calls).toHaveLength(2);
  });

  it('is overridden by providing APP_QUIT, not by a second command', async () => {
    const own: string[] = [];
    const port = { setAppMenu: () => Promise.resolve(), onMenu: () => Promise.resolve(() => undefined), quitApp: () => Promise.reject(new Error('the default')) };
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideShellCore(), provideNativeMenu({ port }), { provide: APP_QUIT, useValue: () => Promise.resolve(void own.push('own')) }],
    });
    await (await TestBed.inject(ShellContextHost).load()).quitApp();
    expect(own).toEqual(['own']);
  });

  it('only warns in a window without a quit', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellCore(), provideShellContext(() => Promise.resolve(() => ({})))] });
    const ctx = await TestBed.inject(ShellContextHost).load();
    await ctx.quitApp();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('APP_QUIT'));
    warn.mockRestore();
  });
});

describe('currentOs', () => {
  it('tells macOS, Windows and the rest apart; the key platform follows', () => {
    expect(currentOs({ platform: 'MacIntel' })).toBe('mac');
    expect(currentOs({ userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' })).toBe('windows');
    expect(currentOs({ platform: 'Linux x86_64' })).toBe('other');
    expect(currentOs(undefined)).toBeDefined();
    expect(currentPlatform({ platform: 'Win32' })).toBe('other');
    expect(currentPlatform({ platform: 'MacIntel' })).toBe('mac');
  });
});

describe('Quit in a Windows in-app menu', () => {
  it('is called Exit there and Quit elsewhere', async () => {
    const titles: Record<string, string | undefined> = {};
    for (const os of ['windows', 'other'] as const) {
      TestBed.resetTestingModule();
      TestBed.configureTestingModule({
        providers: [provideZonelessChangeDetection(), provideShellCore(), provideContributions(QUIT_CONTRIBUTIONS), provideMenus({ appName: 'Demo' }), { provide: MENU_OS, useValue: os }, { provide: MENU_MODE, useValue: 'in-app' }],
      });
      await TestBed.inject(ShellContextHost).load();
      const menus = TestBed.inject(AppMenu).menus();
      const item = menus.flatMap((m) => m.items).find((n) => n.kind === 'item' && n.id === QUIT_COMMAND);
      titles[os] = item?.kind === 'item' ? item.label : undefined;
    }
    expect(titles).toEqual({ windows: 'Exit', other: 'Quit' });
  });
});

describe('Quit with an app-given title', () => {
  it('shows the title the app gives (the menu says what Quit does) and falls back to the plain one when it gives none', async () => {
    const title = signal<string | null>('Quit Demo (keeps running)');
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideShellCore(), provideContributions(QUIT_CONTRIBUTIONS), provideMenus({ appName: 'Demo' }), { provide: MENU_OS, useValue: 'windows' }, { provide: MENU_MODE, useValue: 'in-app' }, { provide: QUIT_TITLE, useValue: title }],
    });
    await TestBed.inject(ShellContextHost).load();
    const menu = TestBed.inject(AppMenu);
    const label = () => {
      const item = menu.menus().flatMap((m) => m.items).find((n) => n.kind === 'item' && n.id === QUIT_COMMAND);
      return item?.kind === 'item' ? item.label : undefined;
    };
    expect(label()).toBe('Quit Demo (keeps running)');
    title.set('Quit Demo');
    expect(label()).toBe('Quit Demo');
    title.set(null);
    expect(label()).toBe('Exit');
  });
});
