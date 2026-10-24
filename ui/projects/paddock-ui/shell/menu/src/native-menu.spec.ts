import { provideZonelessChangeDetection, signal, type WritableSignal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { MENU_EVENT, type NativeMenuModel } from '../generated/menu';
import { AppMenu } from './app-menu';
import { Keymap } from './keymap';
import type { TopMenu } from './menu-model';
import {
  NATIVE_MENU_GATE,
  NATIVE_MENU_PORT,
  NATIVE_MENU_TIMING,
  NativeMenuBridge,
  NO_NATIVE_MENU,
  provideNativeMenu,
  tauriNativeMenuPort,
  toNativeModel,
  type NativeMenuPort,
} from './native-menu';

const MENUS: TopMenu[] = [
  { id: 'edit', label: 'Edit', items: [{ kind: 'item', id: 'pd.edit.undo', label: 'Undo rename', keybinding: 'Mod+Z', enabled: true }] },
  {
    id: 'view',
    label: 'View',
    items: [
      { kind: 'item', id: 'sd.sidebar', label: 'Sidebar', keybinding: 'Mod+B', checked: true, enabled: true },
      { kind: 'separator' },
      { kind: 'submenu', id: 'sd.more', label: 'More', items: [{ kind: 'item', id: 'sd.compact', label: 'Compact', checked: false, enabled: false }] },
    ],
  },
];

describe('the native menu model', () => {
  it('turns the menus into the wire model (no shortcut where the standard Edit items have one)', () => {
    expect(toNativeModel(MENUS, new Set(['pd.edit.undo']))).toEqual({
      menus: [
        { id: 'edit', label: 'Edit', items: [{ kind: 'item', id: 'pd.edit.undo', label: 'Undo rename', keybinding: null, checked: null, enabled: true }] },
        {
          id: 'view',
          label: 'View',
          items: [
            { kind: 'item', id: 'sd.sidebar', label: 'Sidebar', keybinding: 'Mod+B', checked: true, enabled: true },
            { kind: 'separator' },
            { kind: 'submenu', label: 'More', items: [{ kind: 'item', id: 'sd.compact', label: 'Compact', keybinding: null, checked: false, enabled: false }] },
          ],
        },
      ],
    });
    expect(toNativeModel(MENUS).menus[0]!.items[0]).toMatchObject({ keybinding: 'Mod+Z' });
  });

  it('talks to Tauri through the app’s IPC functions', async () => {
    const calls: unknown[] = [];
    let handler: (p: unknown) => void = () => undefined;
    const port = tauriNativeMenuPort({
      invoke: async (cmd, args) => (calls.push([cmd, args]), 'rebuilt'),
      listen: async (event, h) => ((handler = h), calls.push(event), () => undefined),
    }, 'plugin:paddock-shell|menu_set');
    expect(await port.setAppMenu({ menus: [] })).toBe('rebuilt');
    const chosen: string[] = [];
    await port.onMenu((id) => chosen.push(id));
    handler('sd.sidebar');
    handler(42);
    expect(calls).toEqual([['plugin:paddock-shell|menu_set', { menu: { menus: [] } }], MENU_EVENT]);
    expect(chosen).toEqual(['sd.sidebar']);
    expect(await NO_NATIVE_MENU.setAppMenu({ menus: [] })).toBeUndefined();
    (await NO_NATIVE_MENU.onMenu(() => undefined))();
  });
});

/** Timers run by hand (no sleeps): `fire(ms)` runs the live timers set for `ms`. */
function manualTimers() {
  const timers: { fn: () => void; ms: number; live: boolean }[] = [];
  return {
    setTimer: (fn: () => void, ms: number) => {
      const t = { fn, ms, live: true };
      timers.push(t);
      return () => void (t.live = false);
    },
    fire: (ms: number) =>
      timers
        .filter((t) => t.live && t.ms === ms)
        .forEach((t) => {
          t.live = false;
          t.fn();
        }),
  };
}

const settle = () => TestBed.tick();
const flush = () => new Promise((r) => setTimeout(r, 0));

function bridge(port: Partial<NativeMenuPort>, gate: WritableSignal<boolean> = signal(true), mode: 'native' | 'in-app' = 'native') {
  const menus = signal<readonly TopMenu[]>(MENUS);
  const ran: string[] = [];
  const timers = manualTimers();
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: AppMenu, useValue: { menus, run: (id: string) => (ran.push(id), Promise.resolve(true)), mode } },
      { provide: NATIVE_MENU_TIMING, useValue: { debounceMs: 150, firstDebounceMs: 600, maxWaitMs: 3000, setTimer: timers.setTimer } },
      { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: () => Promise.resolve(undefined), notify: () => undefined } },
      provideNativeMenu({ port: { ...NO_NATIVE_MENU, ...port }, gate: () => gate }),
    ],
  });
  const b = TestBed.inject(NativeMenuBridge);
  b.start();
  return { b, menus, ran, timers };
}

describe('NativeMenuBridge', () => {
  it('starts only in native mode', () => {
    const sent: NativeMenuModel[] = [];
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        { provide: AppMenu, useValue: { menus: signal(MENUS), run: () => Promise.resolve(true), mode: 'in-app' } },
        provideNativeMenu({ port: { ...NO_NATIVE_MENU, setAppMenu: async (m) => void sent.push(m) } }),
      ],
    });
    TestBed.inject(AppMenu);
    settle();
    expect(sent).toEqual([]);
    // Without an app context the app is ready at once.
    expect(TestBed.inject(NATIVE_MENU_GATE)()).toBe(true);
  });

  it('waits longer before the first build than between later ones', () => {
    const sent: NativeMenuModel[] = [];
    const { menus, timers, b } = bridge({ setAppMenu: async (m) => void sent.push(m) });
    settle();
    timers.fire(150);
    expect(sent).toHaveLength(0);
    timers.fire(600);
    expect(sent).toHaveLength(1);
    menus.set([MENUS[0]!]);
    settle();
    timers.fire(150);
    expect(sent).toHaveLength(2);
    expect(b.sent()).toBe(2);
  });

  it('builds once at start: nothing before the app is ready, the changes of the start as one', () => {
    const sent: NativeMenuModel[] = [];
    const gate = signal(false);
    const { menus, timers } = bridge({ setAppMenu: async (m) => void sent.push(m) }, gate);
    menus.set([MENUS[0]!]);
    settle();
    timers.fire(600);
    menus.set([...MENUS]);
    settle();
    timers.fire(600);
    window.dispatchEvent(new Event('focus'));
    expect(sent).toHaveLength(0);
    gate.set(true);
    settle();
    menus.set([MENUS[1]!]);
    settle();
    menus.set([...MENUS]);
    settle();
    timers.fire(600);
    timers.fire(150);
    expect(sent).toEqual([toNativeModel(MENUS, new Set(['pd.edit.undo', 'pd.edit.redo']))]);
  });

  it('sends the model once per change and again when the window gets the focus', () => {
    const sent: NativeMenuModel[] = [];
    const { menus, timers } = bridge({ setAppMenu: async (m) => void sent.push(m) });
    settle();
    timers.fire(600);
    menus.set([...MENUS]);
    settle();
    timers.fire(150);
    expect(sent).toHaveLength(1);
    window.dispatchEvent(new Event('focus'));
    expect(sent).toHaveLength(2);
  });

  it('sends the menus after the longest wait when the app never reports ready', () => {
    const sent: NativeMenuModel[] = [];
    const { timers } = bridge({ setAppMenu: async (m) => void sent.push(m) }, signal(false));
    settle();
    timers.fire(600);
    expect(sent).toHaveLength(0);
    timers.fire(3000);
    settle();
    timers.fire(600);
    expect(sent).toHaveLength(1);
  });

  it('retries after a failed update and warns', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    let fail = true;
    const sent: NativeMenuModel[] = [];
    const { timers } = bridge({
      setAppMenu: async (m) => {
        if (fail) throw { code: 'native', message: 'no menu' };
        sent.push(m);
      },
    });
    settle();
    timers.fire(600);
    await flush();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no menu'));
    fail = false;
    window.dispatchEvent(new Event('focus'));
    await flush();
    expect(sent).toHaveLength(1);
    warn.mockRestore();
  });

  it('runs chosen items, but not the echo of a shortcut that just ran the same command', async () => {
    let chosen: (id: string) => void = () => undefined;
    const { ran } = bridge({ onMenu: async (h) => ((chosen = h), () => undefined) });
    await flush();
    chosen('sd.sidebar');
    const keymap = TestBed.inject(Keymap);
    vi.spyOn(keymap, 'ranByKeyWithin').mockImplementation((id) => id === 'sd.compact');
    chosen('sd.compact');
    chosen('sd.other');
    expect(ran).toEqual(['sd.sidebar', 'sd.other']);
  });

  it('degrades when native menu events are unavailable, and unlistens when destroyed', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    bridge({ onMenu: () => Promise.reject(new Error('no events')) });
    await flush();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('native menu events are unavailable'));
    warn.mockRestore();
    TestBed.resetTestingModule();
    const unlisten = vi.fn();
    const { b } = bridge({ onMenu: async () => unlisten });
    b.start();
    await flush();
    TestBed.resetTestingModule();
    expect(unlisten).toHaveBeenCalledTimes(1);
    const late = vi.fn();
    let resolve: (u: () => void) => void = () => undefined;
    bridge({ onMenu: () => new Promise((r) => (resolve = r)) });
    TestBed.resetTestingModule();
    resolve(late);
    await flush();
    expect(late).toHaveBeenCalledTimes(1);
  });
});
