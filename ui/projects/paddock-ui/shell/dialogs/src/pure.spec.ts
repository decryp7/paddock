import { fakeDialogContract, type FakeContractSpec } from '@decryp7/paddock-ui/shell/testing';
import { defineDialog, DIALOG_BASE_COMMANDS, msg, type DialogDef } from '@decryp7/paddock-ui/shell';
import { DIALOG_BOUNDS_KEY, isRememberedPlace, loadDialogBounds, MAX_REMEMBERED, rememberDialogBounds } from './bounds';
import { allowedCommands, commandCovered } from './contracts';
import { DeniedCommands, isDenial } from './denied';
import { dialogKeyAction, findDefaultAction, hasCloseAction } from './keys';
import { DIALOG_MIN, dismissOf, instanceKey, modalityOf, raiseToKindMinimum, resolveSize, TOOL_DEFAULT, VIEWER_MIN } from './kind';
import { ModalGate } from './modal-gate';
import { dialogHash, parseDialogRoute } from './route';
import { TestBed } from '@angular/core/testing';

const load = () => Promise.resolve(class {});
type Spec = Omit<FakeContractSpec<string, string>, 'id'> & { readonly id?: string };
const def = (spec: Spec): DialogDef => defineDialog(fakeDialogContract({ id: 't.d', ...spec }), { title: msg('t.d', 'D'), load }) as DialogDef;

describe('kind policy', () => {
  it('reads the Rust constants of paddock_dialogs::contract from the generated table', () => {
    expect([DIALOG_MIN, VIEWER_MIN, TOOL_DEFAULT]).toEqual([
      { width: 320, height: 200 },
      { width: 680, height: 560 },
      { width: 960, height: 680 },
    ]);
  });

  it('resolves sizes from the contract, never below the kind floor', () => {
    expect(resolveSize(def({ kind: 'decision' }))).toEqual({ width: 400, height: undefined, minWidth: 320, minHeight: 200 });
    expect(resolveSize(def({ kind: 'form', modality: 'modal', width: 'wide' })).width).toBe(720);
    expect(resolveSize(def({ kind: 'viewer', width: 500, height: 300 }))).toEqual({ width: 680, height: 560, minWidth: 680, minHeight: 560 });
    expect(resolveSize(def({ kind: 'tool' }))).toEqual({ width: 960, height: 680, minWidth: 320, minHeight: 200 });
  });

  it('derives modality and dismissal from the kind', () => {
    expect(modalityOf('decision', undefined)).toBe('modal');
    expect(modalityOf('form', undefined)).toBeNull();
    expect(modalityOf('form', 'modeless')).toBe('modeless');
    expect(modalityOf('viewer', 'modal')).toBeNull();
    expect(modalityOf('nonsense' as DialogDef['kind'], undefined)).toBeNull();
    expect([dismissOf('modal'), dismissOf('modeless')]).toEqual(['cancel', 'close']);
  });

  it('raises a remembered size to the floor and keys instances by input', () => {
    expect(raiseToKindMinimum('viewer', { dx: 1, dy: 2, width: 10, height: 10 })).toEqual({ dx: 1, dy: 2, width: 680, height: 560 });
    const tool = { dx: 0, dy: 0, width: 10, height: 10 };
    expect(raiseToKindMinimum('tool', tool)).toBe(tool);
    expect(instanceKey({ a: 1 })).toBe(instanceKey({ a: 1 }));
    expect(instanceKey({ a: 1 })).not.toBe(instanceKey({ a: 2 }));
    expect(instanceKey(undefined)).toMatch(/^[a-z0-9]+$/);
  });
});

describe('contract commands', () => {
  it('covers exact names and prefixes, and lists what a dialog may call', () => {
    expect(commandCovered(['a', 'ui_*'], 'ui_theme')).toBe(true);
    expect(commandCovered(['a', 'ui_*'], 'b')).toBe(false);
    expect(allowedCommands(def({ kind: 'tool', commands: ['x'] }), ['y'])).toEqual([...DIALOG_BASE_COMMANDS, 'y', 'x']);
    expect(allowedCommands({ commands: [] })).toEqual([...DIALOG_BASE_COMMANDS]);
  });

  it('makes test contracts the way Rust fills them', () => {
    expect(fakeDialogContract({ id: 'a.b', kind: 'form' })).toEqual({ id: 'a.b', kind: 'form', modality: 'modal', width: 560, widthStep: 'medium', height: null, instanced: false, requiredChoice: false, commands: [] });
    expect(fakeDialogContract({ id: 'a.t', kind: 'tool', width: 820, height: 640 })).toMatchObject({ width: 820, widthStep: null, height: 640 });
    expect(fakeDialogContract({ id: 'a.t', kind: 'tool' })).toMatchObject({ width: 960, widthStep: null, height: 680 });
  });
});

describe('dialog route', () => {
  it('parses what Rust builds and refuses anything else', () => {
    const route = { id: 'sd.about', host: 'main', label: 'd-main-1' };
    expect(parseDialogRoute(dialogHash(route))).toEqual(route);
    expect(dialogHash(route, { title: 'A b' })).toContain('title=A+b');
    for (const bad of [null, undefined, '', '#/main', '#/dialog?id=X&host=main&label=d-main-1', '#/dialog?id=a&host=main&label=w-1', '#/dialog?id=a&host=&label=d-1']) {
      expect(parseDialogRoute(bad)).toBeNull();
    }
  });
});

describe('dialog keys', () => {
  const key = (k: string, extra: Partial<Omit<KeyboardEvent, 'target'>> & { target?: unknown } = {}) => ({ key: k, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, target: null, ...extra });
  it('dismisses on Escape and Cmd/Ctrl+W, runs the default on Enter outside controls with their own Enter', () => {
    expect(dialogKeyAction(key('Escape'))).toBe('dismiss');
    expect(dialogKeyAction(key('Escape', { altKey: true }))).toBeNull();
    expect(dialogKeyAction(key('w', { metaKey: true }))).toBe('dismiss');
    expect(dialogKeyAction(key('W', { ctrlKey: true }))).toBe('dismiss');
    expect(dialogKeyAction(key('w', { ctrlKey: true, shiftKey: true }))).toBeNull();
    expect(dialogKeyAction(key('Enter', { target: document.createElement('input') }))).toBe('default');
    expect(dialogKeyAction(key('Enter', { target: document.createElement('textarea') }))).toBeNull();
    const option = document.createElement('div');
    option.setAttribute('role', 'option');
    expect(dialogKeyAction(key('Enter', { target: option }))).toBeNull();
    expect(dialogKeyAction(key('Enter', { target: { isContentEditable: true } }))).toBeNull();
    expect(dialogKeyAction(key('Enter', { shiftKey: true }))).toBeNull();
    expect(dialogKeyAction(key('Escape', { isComposing: true }))).toBeNull();
    expect(dialogKeyAction(key('Escape', { defaultPrevented: true }))).toBeNull();
    expect(dialogKeyAction(key('a'))).toBeNull();
  });

  it('finds the default action and a close action', () => {
    const root = document.createElement('div');
    root.innerHTML = `<button class="primary" disabled>No</button><button class="primary" id="p">Go</button><footer><button>Close</button></footer>`;
    expect(findDefaultAction(root)?.id).toBe('p');
    root.insertAdjacentHTML('beforeend', '<button data-default id="d">D</button>');
    expect(findDefaultAction(root)?.id).toBe('d');
    expect(hasCloseAction(root)).toBe(true);
    expect(hasCloseAction(root, root.querySelector('footer'))).toBe(false);
    root.insertAdjacentHTML('beforeend', '<span data-dialog-close></span>');
    expect(hasCloseAction(root, root.querySelector('footer'))).toBe(true);
  });
});

describe('remembered bounds', () => {
  const store = () => {
    const data = new Map<string, string>();
    return { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v), data };
  };
  const place = { dx: 10, dy: 20, width: 700, height: 600 };

  it('remembers per dialog and survives missing, corrupt and foreign data', () => {
    const s = store();
    expect(loadDialogBounds(s)).toEqual({});
    rememberDialogBounds('a.b', place, s);
    expect(loadDialogBounds(s)).toEqual({ 'a.b': place });
    rememberDialogBounds('a.b', { ...place, width: 0 }, s);
    expect(loadDialogBounds(s)['a.b']).toEqual(place);
    s.data.set(DIALOG_BOUNDS_KEY, '{nope');
    expect(loadDialogBounds(s)).toEqual({});
    s.data.set(DIALOG_BOUNDS_KEY, JSON.stringify({ version: 99, bounds: {} }));
    expect(loadDialogBounds(s)).toEqual({});
    s.data.set(DIALOG_BOUNDS_KEY, JSON.stringify({ version: 1, bounds: { x: place, y: { dx: 'a' } } }));
    expect(Object.keys(loadDialogBounds(s))).toEqual(['x']);
    expect(loadDialogBounds(null)).toEqual({});
    expect(isRememberedPlace(null)).toBe(false);
  });

  it('keeps at most MAX_REMEMBERED, the latest last, and never throws', () => {
    const s = store();
    for (let i = 0; i < MAX_REMEMBERED + 5; i++) rememberDialogBounds(`d.n${i}`, place, s);
    const kept = Object.keys(loadDialogBounds(s));
    expect(kept).toHaveLength(MAX_REMEMBERED);
    expect(kept.at(-1)).toBe(`d.n${MAX_REMEMBERED + 4}`);
    expect(() => rememberDialogBounds('x.y', place, { getItem: () => null, setItem: () => { throw new Error('full'); } })).not.toThrow();
    rememberDialogBounds('x.y', place);
    expect(loadDialogBounds()['x.y']).toEqual(place);
    localStorage.clear();
  });
});

describe('modal gate and denials', () => {
  it('counts modal dialogs and never goes below zero', () => {
    const gate = TestBed.inject(ModalGate);
    gate.leave();
    expect(gate.blocked()).toBe(false);
    gate.enter();
    gate.enter();
    gate.leave();
    expect(gate.blocked()).toBe(true);
    gate.leave();
    expect(gate.blocked()).toBe(false);
  });

  it('records the guard refusals only', () => {
    const denied = TestBed.inject(DeniedCommands);
    const spy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    expect(denied.report(new Error('x'))).toBe(false);
    expect(denied.report({ code: 'limit', command: 'a', message: 'm' })).toBe(false);
    expect(denied.report({ code: 'denied', command: 'a', message: 'a is not available', retryable: false })).toBe(true);
    expect(denied.report({ code: 'denied', command: 'b', message: 'b is not available', retryable: false, dialog: 'sd.about' })).toBe(true);
    expect(denied.last()?.command).toBe('b');
    expect(isDenial(null)).toBe(false);
    expect(spy).toHaveBeenCalledTimes(2);
    spy.mockRestore();
  });
});

describe('debounced', () => {
  it('runs once after the last ask, and can be cancelled', async () => {
    const { debounced } = await import('./dialog-shell');
    vi.useFakeTimers();
    try {
      let runs = 0;
      const d = debounced(() => runs++, 60);
      d.ask();
      vi.advanceTimersByTime(59);
      d.ask();
      vi.advanceTimersByTime(59);
      expect(runs).toBe(0);
      vi.advanceTimersByTime(1);
      expect(runs).toBe(1);
      d.ask();
      d.cancel();
      d.cancel();
      vi.advanceTimersByTime(120);
      expect(runs).toBe(1);
    } finally {
      vi.useRealTimers();
    }
  });
});
