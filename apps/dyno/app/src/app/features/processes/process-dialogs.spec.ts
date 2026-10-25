import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Dialogs } from '../../shared/dialog';
import { provideIcons } from '../../shared/icons';
import { ProcessesStore } from '../../state/processes.store';
import { AffinityDialog, CapDialog, LockersDialog } from './process-dialogs';

function setup<T>(component: new () => T, data: unknown, backend = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 })) {
  const close = vi.fn();
  TestBed.configureTestingModule({
    providers: [provideIcons(), { provide: BACKEND, useValue: backend }, { provide: DIALOG_DATA, useValue: data }, { provide: DialogRef, useValue: { close } }],
  });
  const f = TestBed.createComponent(component);
  f.detectChanges();
  return { f, close, el: f.nativeElement as HTMLElement, backend };
}
const btn = (el: HTMLElement, label: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;

describe('AffinityDialog', () => {
  it('checks the cores of the mask and applies the new mask', () => {
    const { f, close, el } = setup(AffinityDialog, { name: 'rustc', cores: 4, mask: 0b0101 });
    expect(el.textContent).toContain('rustc');
    const boxes = [...el.querySelectorAll<HTMLInputElement>('input[type=checkbox]')];
    expect(boxes.map((b) => b.checked)).toEqual([true, false, true, false]);
    expect(el.textContent).toContain('Core 1');
    boxes[1]!.dispatchEvent(new Event('change'));
    f.detectChanges();
    btn(el, 'Apply').click();
    expect(close).toHaveBeenCalledWith(0b0111);
  });
  it('unchecking works; All and None; Apply is disabled with no core', () => {
    const { f, close, el } = setup(AffinityDialog, { name: 'x', cores: 3, mask: null });
    expect([...el.querySelectorAll<HTMLInputElement>('input')].every((b) => b.checked)).toBe(true);
    el.querySelectorAll('input')[0]!.dispatchEvent(new Event('change'));
    f.detectChanges();
    btn(el, 'Apply').click();
    expect(close).toHaveBeenLastCalledWith(0b110);
    btn(el, 'None').click();
    f.detectChanges();
    expect(btn(el, 'Apply').disabled).toBe(true);
    btn(el, 'All').click();
    f.detectChanges();
    expect(btn(el, 'Apply').disabled).toBe(false);
    btn(el, 'Apply').click();
    expect(close).toHaveBeenLastCalledWith(7);
    btn(el, 'Cancel').click();
    expect(close).toHaveBeenLastCalledWith();
  });
});

describe('CapDialog', () => {
  it('applies the entered cap and validates the range', async () => {
    const { f, close, el } = setup(CapDialog, 'cargo');
    expect(el.textContent).toContain('cargo');
    const input = el.querySelector<HTMLInputElement>('input')!;
    await f.whenStable();
    btn(el, 'Apply cap').click();
    expect(close).toHaveBeenLastCalledWith(50);
    input.value = '99';
    input.dispatchEvent(new Event('input'));
    f.detectChanges();
    await f.whenStable();
    f.detectChanges();
    expect(btn(el, 'Apply cap').disabled).toBe(true);
    input.value = '30';
    input.dispatchEvent(new Event('input'));
    f.detectChanges();
    await f.whenStable();
    f.detectChanges();
    btn(el, 'Apply cap').click();
    expect(close).toHaveBeenLastCalledWith(30);
    btn(el, 'Cancel').click();
    expect(close).toHaveBeenLastCalledWith();
  });
});

describe('LockersDialog', () => {
  async function typePath(f: { detectChanges(): void; componentInstance: unknown }, _el: HTMLElement, value: string) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (f.componentInstance as any).path.set(value);
    f.detectChanges();
  }
  it('Find is disabled for an empty path', async () => {
    const { el } = setup(LockersDialog, undefined);
    expect(btn(el, 'Find').disabled).toBe(true);
  });
  it('lists the lockers of a path', async () => {
    const { f, el } = setup(LockersDialog, undefined);
    await typePath(f, el, '/work/target/debug');
    el.querySelector('form')!.dispatchEvent(new Event('submit'));
    await f.whenStable();
    await new Promise((r) => setTimeout(r, 5));
    f.detectChanges();
    expect(el.querySelectorAll('li').length).toBeGreaterThan(0);
    expect(el.textContent).toContain('Writes build output');
  });
  it('says so when nothing has the path open', async () => {
    const { f, el } = setup(LockersDialog, undefined);
    await typePath(f, el, '/nothing/here.txt');
    el.querySelector('form')!.dispatchEvent(new Event('submit'));
    await new Promise((r) => setTimeout(r, 5));
    f.detectChanges();
    expect(el.textContent).toContain('Nothing has it open right now.');
  });
  it('shows an error when the lookup fails', async () => {
    const backend = new MockBackend({ latencyMs: 0, fail: new Set(['lockers']) });
    const { f, el } = setup(LockersDialog, undefined, backend);
    await typePath(f, el, '/x');
    el.querySelector('form')!.dispatchEvent(new Event('submit'));
    await new Promise((r) => setTimeout(r, 5));
    f.detectChanges();
    expect(el.querySelector('[role=alert]')).not.toBeNull();
  });
  it('End asks first; cancelled does nothing, confirmed acts and searches again', async () => {
    const { f, el } = setup(LockersDialog, undefined);
    await typePath(f, el, '/work/target');
    el.querySelector('form')!.dispatchEvent(new Event('submit'));
    await new Promise((r) => setTimeout(r, 5));
    f.detectChanges();
    const store = TestBed.inject(ProcessesStore);
    const act = vi.spyOn(store, 'act');
    const confirm = vi.spyOn(TestBed.inject(Dialogs), 'confirm');
    confirm.mockResolvedValueOnce(false);
    el.querySelector<HTMLButtonElement>('li button')!.click();
    await new Promise((r) => setTimeout(r, 5));
    expect(act).not.toHaveBeenCalled();
    confirm.mockResolvedValueOnce(true);
    el.querySelector<HTMLButtonElement>('li button')!.click();
    await new Promise((r) => setTimeout(r, 10));
    expect(act).toHaveBeenCalledWith(expect.objectContaining({ groupId: null }), 'end');
  });
});
