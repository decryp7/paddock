import { provideZonelessChangeDetection, signal, type WritableSignal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { WindowReady } from '@decryp7/paddock-ui/shell/startup';
import { provideShellContext, SHELL_IDLE, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { MENU_EVENT, type NativeMenuModel } from '../generated/menu';
import { AppMenu } from './app-menu';
import { Keymap } from './keymap';
import type { TopMenu } from './menu-model';
import {
  NATIVE_MENU_GATE,
  NATIVE_MENU_SCHEDULE,
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

/** Idle callbacks run by hand (no sleeps): `idle()` runs the work scheduled so far. */
function manualIdle() {
  let queue: { work: () => void; live: boolean }[] = [];
  return {
    schedule: (work: () => void) => {
      const w = { work, live: true };
      queue.push(w);
      return () => void (w.live = false);
    },
    idle: () => {
      const now = queue;
      queue = [];
      for (const w of now) if (w.live) w.work();
    },
  };
}

const settle = () => TestBed.tick();
const flush = () => new Promise((r) => setTimeout(r, 0));

function bridge(port: Partial<NativeMenuPort>, gate: WritableSignal<boolean> = signal(true)) {
  const menus = signal<readonly TopMenu[]>(MENUS);
  const ran: string[] = [];
  const timers = manualIdle();
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: AppMenu, useValue: { menus, run: (id: string) => (ran.push(id), Promise.resolve(true)), mode: 'native' } },
      { provide: NATIVE_MENU_SCHEDULE, useValue: timers.schedule },
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
  });

  it('opens the default gate once the window is ready and the app context has loaded or failed to (at once without one)', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    expect(TestBed.inject(NATIVE_MENU_GATE)(), 'not before the window reported ready').toBe(false);
    TestBed.inject(WindowReady).markReady();
    expect(TestBed.inject(NATIVE_MENU_GATE)()).toBe(true);
    for (const load of [() => Promise.resolve(() => ({})), () => Promise.reject(new Error('chunk failed'))]) {
      TestBed.resetTestingModule();
      let idle: () => void = () => undefined;
      TestBed.configureTestingModule({
        providers: [
          provideZonelessChangeDetection(),
          provideShellContext(load, { prefetch: 'first-use' }),
          { provide: SHELL_IDLE, useValue: (w: () => void) => (idle = w) },
        ],
      });
      const gate = TestBed.inject(NATIVE_MENU_GATE);
      TestBed.inject(WindowReady).markReady();
      expect(gate()).toBe(false);
      idle();
      await flush();
      expect(gate()).toBe(true);
    }
  });

  it('the default schedule runs once when idle unless cancelled', () => {
    const idles: (() => void)[] = [];
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: SHELL_IDLE, useValue: (w: () => void) => idles.push(w) }] });
    const schedule = TestBed.inject(NATIVE_MENU_SCHEDULE);
    const ran: string[] = [];
    schedule(() => ran.push('a'));
    schedule(() => ran.push('b'))();
    idles.forEach((w) => w());
    expect(ran).toEqual(['a']);
  });

  it('builds once at start: nothing before the app is ready, the changes of the start as one', () => {
    const sent: NativeMenuModel[] = [];
    const gate = signal(false);
    const listened = vi.fn(async () => () => undefined);
    const { menus, timers, b } = bridge({ setAppMenu: async (m) => void sent.push(m), onMenu: listened }, gate);
    menus.set([MENUS[0]!]);
    settle();
    timers.idle();
    menus.set([...MENUS]);
    settle();
    timers.idle();
    window.dispatchEvent(new Event('focus'));
    expect(sent).toHaveLength(0);
    expect(listened).not.toHaveBeenCalled();
    gate.set(true);
    settle();
    expect(listened).toHaveBeenCalledTimes(1);
    menus.set([MENUS[1]!]);
    settle();
    menus.set([...MENUS]);
    settle();
    timers.idle();
    expect(sent).toEqual([toNativeModel(MENUS, new Set(['pd.edit.undo', 'pd.edit.redo']))]);
    expect(b.sent()).toBe(1);
  });

  it('sends the model once per change and again when the window gets the focus', () => {
    const sent: NativeMenuModel[] = [];
    const { menus, timers } = bridge({ setAppMenu: async (m) => void sent.push(m) });
    settle();
    timers.idle();
    menus.set([...MENUS]);
    settle();
    timers.idle();
    expect(sent).toHaveLength(1);
    menus.set([MENUS[0]!]);
    settle();
    timers.idle();
    expect(sent).toHaveLength(2);
    window.dispatchEvent(new Event('focus'));
    expect(sent).toHaveLength(3);
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
    timers.idle();
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
    settle();
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
    settle();
    await flush();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('native menu events are unavailable'));
    warn.mockRestore();
    TestBed.resetTestingModule();
    const unlisten = vi.fn();
    const { b } = bridge({ onMenu: async () => unlisten });
    b.start();
    settle();
    await flush();
    TestBed.resetTestingModule();
    expect(unlisten).toHaveBeenCalledTimes(1);
    const late = vi.fn();
    let resolve: (u: () => void) => void = () => undefined;
    bridge({ onMenu: () => new Promise((r) => (resolve = r)) });
    settle();
    TestBed.resetTestingModule();
    resolve(late);
    await flush();
    expect(late).toHaveBeenCalledTimes(1);
  });
});
