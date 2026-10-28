import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { LockersDialog } from './process-dialogs';

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
});
