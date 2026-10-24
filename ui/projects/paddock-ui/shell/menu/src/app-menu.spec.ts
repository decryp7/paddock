import { provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineCommand, defineKeybinding, defineMenuItem, msg, provideContributions, provideShellContext, SHELL_IDLE, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { AppMenu, isTauri, MENU_MODE, provideMenus } from './app-menu';
import { Keymap, KEY_PLATFORM, KEYBINDING_STORAGE } from './keymap';
import { defineSubmenu, provideSubmenus } from './submenus';

interface Ctx {
  readonly wrap: () => boolean;
}
const wrap = signal(false);
const ran: string[] = [];

const CONTRIBUTIONS = {
  commands: [
    defineCommand<Ctx>({ id: 'sd.wrap', title: msg('sd.wrap', 'Word wrap'), checked: (c) => c.wrap(), run: () => void ran.push('sd.wrap') }),
    defineCommand<Ctx>({ id: 'sd.greet', title: msg('sd.greet', 'Say hello'), enabled: (c) => !c.wrap(), run: () => void ran.push('sd.greet') }),
    defineCommand<Ctx>({ id: 'sd.about', title: msg('sd.about', 'About Demo'), run: () => undefined }),
    defineCommand<Ctx>({ id: 'sd.boom', title: msg('sd.boom', 'Boom'), when: () => { throw new Error('x'); }, run: () => undefined }),
  ],
  keybindings: [defineKeybinding({ command: 'sd.greet', key: 'Mod+Shift+H' })],
  menus: [
    defineMenuItem<Ctx>({ menu: 'view', command: 'sd.wrap', group: 'text', order: 100 }),
    defineMenuItem<Ctx>({ menu: 'help', command: 'sd.greet', order: 100 }),
    defineMenuItem<Ctx>({ menu: 'help', command: 'sd.boom', order: 200 }),
    defineMenuItem<Ctx>({ menu: 'app', command: 'sd.about', order: 100 }),
    defineMenuItem<Ctx>({ menu: 'sd.more', command: 'sd.about', order: 100, when: () => { throw new Error('y'); } }),
    defineMenuItem<Ctx>({ menu: 'sd.ctx', command: 'sd.greet', order: 1 }),
  ],
};

function setup(withContext = true, mode: 'in-app' | 'native' = 'in-app') {
  ran.length = 0;
  wrap.set(false);
  let idle: () => void = () => undefined;
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      provideContributions<Ctx>(CONTRIBUTIONS),
      provideSubmenus(defineSubmenu({ id: 'sd.more', menu: 'view', title: msg('sd.more', 'More'), order: 200 })),
      provideMenus({ appName: 'Demo' }),
      ...(withContext ? [provideShellContext<Ctx>(() => Promise.resolve(() => ({ wrap })))] : []),
      { provide: SHELL_IDLE, useValue: (w: () => void) => (idle = w) },
      { provide: KEY_PLATFORM, useValue: 'other' },
      { provide: MENU_MODE, useValue: mode },
      { provide: KEYBINDING_STORAGE, useValue: null },
      { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: () => Promise.resolve(undefined), notify: () => undefined } },
    ],
  });
  return { app: TestBed.inject(AppMenu), idle: () => idle() };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe('AppMenu', () => {
  it('defaults to the in-app menu bar outside the macOS desktop app', () => {
    const { app } = setup();
    expect(app.mode).toBe('in-app');
    expect(isTauri({})).toBe(false);
    expect(isTauri({ __TAURI_INTERNALS__: {} })).toBe(true);
    expect(TestBed.inject(MENU_MODE)).toBe('in-app');
  });

  it('builds the menus from the contributions with live states and effective keys', async () => {
    const { app, idle } = setup();
    // Before the app's context is loaded every command is offered with its defaults.
    // In the window the app menu's entries are under File: the title bar already shows the app's name (regression:
    // "Demo" was shown twice, as the title and as the first menu).
    expect(app.menus().map((m) => m.label)).toEqual(['File', 'View', 'Help']);
    idle();
    await flush();
    expect(app.menus()).toEqual([
      { id: 'file', label: 'File', items: [{ kind: 'item', id: 'sd.about', label: 'About Demo', enabled: true }] },
      { id: 'view', label: 'View', items: [{ kind: 'item', id: 'sd.wrap', label: 'Word wrap', enabled: true, checked: false }] },
      { id: 'help', label: 'Help', items: [{ kind: 'item', id: 'sd.greet', label: 'Say hello', enabled: true, keybinding: 'Mod+Shift+H' }] },
    ]);
    wrap.set(true);
    expect(app.menus()[1]!.items[0]).toMatchObject({ checked: true });
    expect(app.menus()[2]!.items[0]).toMatchObject({ enabled: false });
    TestBed.inject(Keymap).bind('sd.greet', 'Mod+G');
    expect(app.menus()[2]!.items[0]).toMatchObject({ keybinding: 'Mod+G' });
    expect(app.items('sd.ctx')).toEqual([{ kind: 'item', id: 'sd.greet', label: 'Say hello', enabled: false, keybinding: 'Mod+G' }]);
    expect(app.items('sd.none')).toEqual([]);
  });

  it('the macOS menu bar keeps the app menu, titled with the app name', () => {
    const { app } = setup(true, 'native');
    expect(app.menus().map((m) => [m.id, m.label])).toEqual([
      ['app', 'Demo'],
      ['view', 'View'],
      ['help', 'Help'],
    ]);
  });

  it('runs items through the commands; disabled and unknown ones do nothing', async () => {
    const { app } = setup(false);
    expect(await app.run('sd.wrap')).toBe(true);
    // Without the app's part of the context its `enabled` throws: disabled, not run.
    expect(await app.run('sd.greet')).toBe(false);
    expect(await app.run('sd.nope')).toBe(false);
    expect(ran).toEqual(['sd.wrap']);
  });
});
