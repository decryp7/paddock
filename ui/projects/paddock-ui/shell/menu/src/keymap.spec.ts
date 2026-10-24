import { provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineCommand, defineKeybinding, msg, provideContributions, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import {
  activeScopes,
  Keymap,
  KEY_PLATFORM,
  KEYBINDING_STORAGE,
  KEYBINDINGS_KEY,
  loadOverrides,
  provideKeyboardShortcuts,
  RESERVED_KEYS,
  saveOverrides,
  type KeybindingStorage,
} from './keymap';

class MemoryStorage implements KeybindingStorage {
  readonly data = new Map<string, string>();
  fail = false;
  getItem(k: string) {
    return this.data.get(k) ?? null;
  }
  setItem(k: string, v: string) {
    if (this.fail) throw new Error('quota');
    this.data.set(k, v);
  }
  removeItem(k: string) {
    this.data.delete(k);
  }
}

const ran: string[] = [];
const enabled = signal(true);
const COMMANDS = [
  defineCommand({ id: 'sd.greet', title: msg('sd.greet', 'Greet'), run: () => void ran.push('sd.greet') }),
  defineCommand({ id: 'sd.page', title: msg('sd.page', 'Page'), run: () => void ran.push('sd.page') }),
  defineCommand({ id: 'sd.off', title: msg('sd.off', 'Off'), enabled: () => enabled(), run: () => void ran.push('sd.off') }),
  defineCommand({ id: 'sd.hidden', title: msg('sd.hidden', 'Hidden'), when: () => false, run: () => void ran.push('sd.hidden') }),
  defineCommand({ id: 'sd.free', title: msg('sd.free', 'Free'), run: () => void ran.push('sd.free') }),
];
const KEYS = [
  defineKeybinding({ command: 'sd.greet', key: 'Mod+Shift+H' }),
  defineKeybinding({ command: 'sd.page', key: 'Mod+Shift+H', scope: 'page.home' }),
  defineKeybinding({ command: 'sd.off', key: 'Mod+O' }),
  defineKeybinding({ command: 'sd.hidden', key: 'Mod+J' }),
  defineKeybinding({ command: 'sd.greet', key: 'F2' }),
];

function setup(storage: KeybindingStorage | null = new MemoryStorage()) {
  ran.length = 0;
  enabled.set(true);
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      provideContributions({ commands: COMMANDS, keybindings: KEYS }),
      provideKeyboardShortcuts(),
      { provide: KEY_PLATFORM, useValue: 'other' },
      { provide: KEYBINDING_STORAGE, useValue: storage },
      { provide: RESERVED_KEYS, useValue: [{ key: 'F6', description: 'Next region' }] },
      { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: () => Promise.resolve(undefined), notify: () => undefined } },
    ],
  });
  return TestBed.inject(Keymap);
}

function press(target: EventTarget, init: KeyboardEventInit): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(e);
  return e;
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe('Keymap', () => {
  afterEach(() => document.body.replaceChildren());

  it('runs the command of a key press in the innermost scope, only when offered and enabled', async () => {
    const keymap = setup();
    const e = press(document.body, { key: 'H', ctrlKey: true, shiftKey: true });
    await flush();
    expect(ran).toEqual(['sd.greet']);
    expect(e.defaultPrevented).toBe(true);
    const page = document.createElement('section');
    page.setAttribute('data-pd-key-scope', 'page.home');
    const button = page.appendChild(document.createElement('button'));
    document.body.append(page);
    press(button, { key: 'H', ctrlKey: true, shiftKey: true });
    await flush();
    expect(ran).toEqual(['sd.greet', 'sd.page']);
    enabled.set(false);
    expect(press(document.body, { key: 'o', ctrlKey: true }).defaultPrevented).toBe(false);
    expect(press(document.body, { key: 'j', ctrlKey: true }).defaultPrevented).toBe(false);
    expect(press(document.body, { key: 'q', ctrlKey: true }).defaultPrevented).toBe(false);
    expect(press(document.body, { key: 'Shift', shiftKey: true }).defaultPrevented).toBe(false);
    expect(keymap.commandForKey({ key: 'F2', code: 'F2', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false })).toBe('sd.greet');
    expect(keymap.ranByKeyWithin('sd.page', 400)).toBe(true);
    expect(keymap.ranByKeyWithin('sd.greet', 400)).toBe(false);
    expect(keymap.ranByKeyWithin('sd.page', 400, Date.now() + 1000)).toBe(false);
  });

  it('leaves typing and editing keys to text fields, suspends while recording, and skips handled events', async () => {
    const keymap = setup();
    const input = document.body.appendChild(document.createElement('input'));
    press(input, { key: 'F2' });
    press(input, { key: 'H', ctrlKey: true, shiftKey: true });
    await flush();
    expect(ran).toEqual(['sd.greet', 'sd.greet']);
    keymap.bind('sd.free', 'Mod+Z');
    press(input, { key: 'z', ctrlKey: true });
    await flush();
    expect(ran).toHaveLength(2);
    keymap.suspend();
    expect(keymap.suspended()).toBe(true);
    press(document.body, { key: 'F2' });
    keymap.resume();
    const handled = new KeyboardEvent('keydown', { key: 'F2', cancelable: true });
    handled.preventDefault();
    expect(keymap.dispatch(handled)).toBeNull();
    await flush();
    expect(ran).toHaveLength(2);
  });

  it('binds, refuses invalid and taken keys unless forced, and resets', () => {
    const storage = new MemoryStorage();
    const keymap = setup(storage);
    expect(keymap.keyFor('sd.greet')).toBe('Mod+Shift+H');
    expect(keymap.keyFor('sd.greet', 'page.home')).toBeUndefined();
    expect(keymap.declared('sd.greet')).toEqual(['Mod+Shift+H', 'F2']);
    expect(keymap.bind('sd.free', 'Hyper+K')).toEqual({ ok: false, reason: 'invalid' });
    const taken = keymap.bind('sd.free', 'ctrl+shift+h');
    expect(taken.ok).toBe(false);
    expect(taken.ok === false && taken.reason === 'conflict' && taken.commands.map((c) => c.command)).toEqual(['sd.greet']);
    const reserved = keymap.bind('sd.free', 'F6');
    expect(reserved.ok === false && reserved.reason === 'conflict' && reserved.reserved.map((r) => r.description)).toEqual(['Next region']);
    expect(keymap.conflicts()).toEqual([]);
    expect(keymap.bind('sd.free', 'Mod+Shift+H', { force: true })).toEqual({ ok: true });
    expect(keymap.conflicts().map((c) => [c.first.command, c.second.command])).toEqual([['sd.greet', 'sd.free']]);
    expect(keymap.bind('sd.greet', null)).toEqual({ ok: true });
    expect(keymap.keyFor('sd.greet')).toBeUndefined();
    expect(keymap.conflicts()).toEqual([]);
    expect(JSON.parse(storage.getItem(KEYBINDINGS_KEY)!)).toEqual({ version: 1, overrides: { 'sd.free': 'Mod+Shift+H', 'sd.greet': null } });
    keymap.reset('sd.greet');
    expect(keymap.keyFor('sd.greet')).toBe('Mod+Shift+H');
    keymap.resetAll();
    expect(storage.getItem(KEYBINDINGS_KEY)).toBeNull();
  });

  it('keeps overrides across a restart and follows another window’s changes', async () => {
    const storage = new MemoryStorage();
    setup(storage).bind('sd.greet', 'Mod+G');
    TestBed.resetTestingModule();
    const keymap = setup(storage);
    expect(keymap.keyFor('sd.greet')).toBe('Mod+G');
    press(document.body, { key: 'g', ctrlKey: true });
    await flush();
    expect(ran).toEqual(['sd.greet']);
    storage.data.set(KEYBINDINGS_KEY, JSON.stringify({ version: 1, overrides: { 'sd.greet': 'Mod+K' } }));
    window.dispatchEvent(new StorageEvent('storage', { key: KEYBINDINGS_KEY }));
    expect(keymap.keyFor('sd.greet')).toBe('Mod+K');
    window.dispatchEvent(new StorageEvent('storage', { key: 'other' }));
    expect(keymap.keyFor('sd.greet')).toBe('Mod+K');
  });

  it('degrades when the storage is missing, corrupt, of another version or full', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const storage = new MemoryStorage();
    storage.data.set(KEYBINDINGS_KEY, '{not json');
    expect(loadOverrides(storage)).toEqual({});
    storage.data.set(KEYBINDINGS_KEY, JSON.stringify({ version: 2, overrides: { 'sd.greet': 'F1' } }));
    expect(loadOverrides(storage)).toEqual({});
    expect(loadOverrides(null)).toEqual({});
    expect(saveOverrides({ a: 'F1' }, null)).toBe(false);
    storage.fail = true;
    const keymap = setup(storage);
    expect(keymap.bind('sd.greet', 'Mod+G')).toEqual({ ok: true });
    expect(keymap.keyFor('sd.greet')).toBe('Mod+G');
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('could not be stored'));
    TestBed.resetTestingModule();
    expect(setup(null).bind('sd.greet', 'Mod+G')).toEqual({ ok: true });
    warn.mockRestore();
  });

  it('lists the active scopes innermost first', () => {
    const outer = document.createElement('div');
    outer.setAttribute('data-pd-key-scope', 'page.home');
    const inner = outer.appendChild(document.createElement('div'));
    inner.setAttribute('data-pd-key-scope', 'dialog.x');
    const leaf = inner.appendChild(document.createElement('span'));
    expect(activeScopes(leaf)).toEqual(['dialog.x', 'page.home', 'window']);
    expect(activeScopes(null)).toEqual(['window']);
    expect(activeScopes(window)).toEqual(['window']);
  });
});
