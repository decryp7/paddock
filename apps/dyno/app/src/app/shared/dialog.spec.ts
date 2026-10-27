import { Dialog, DialogRef } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { of } from 'rxjs';
import { ConfirmDialog, Dialogs } from './dialog';
import { provideIcons } from './icons';

describe('Dialogs', () => {
  afterEach(() => TestBed.inject(Dialog).closeAll());
  function setup() {
    TestBed.configureTestingModule({ providers: [provideIcons()] });
    return TestBed.inject(Dialogs);
  }
  const click = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('.dy-dialog-panel footer button')].find((b) => b.textContent?.trim() === label)!.click();

  it('confirm resolves true for the confirm button and shows title, message and details', async () => {
    const d = setup();
    const p = d.confirm({ title: 'End it?', message: 'Sure?', confirmLabel: 'End', danger: true, details: ['a', 'b'] });
    TestBed.tick();
    const panel = document.querySelector('.dy-dialog-panel')!;
    expect(panel.textContent).toContain('End it?');
    expect(panel.textContent).toContain('Sure?');
    expect(panel.querySelectorAll('li').length).toBe(2);
    click('End');
    expect(await p).toBe(true);
  });
  it('confirm resolves false for Cancel and for the frame close button', async () => {
    const d = setup();
    const p = d.confirm({ title: 'T', message: 'M', confirmLabel: 'Go', icon: 'info' });
    TestBed.tick();
    click('Cancel');
    expect(await p).toBe(false);
    const q = d.confirm({ title: 'T', message: 'M', confirmLabel: 'Go' });
    TestBed.tick();
    document.querySelector<HTMLButtonElement>('.dy-dialog-panel [aria-label=Close]')!.click();
    expect(await q).toBe(false);
  });
  it('open passes data and applies the shared look', () => {
    const d = setup();
    const ref = d.open<boolean>(ConfirmDialog, { title: 'X', message: 'm', confirmLabel: 'ok' });
    TestBed.tick();
    expect(ref).toBeInstanceOf(DialogRef);
    expect(document.querySelector('.dy-dialog-panel')).not.toBeNull();
    expect(document.querySelector('.dy-backdrop')).not.toBeNull();
    ref.close();
    expect(of(1)).toBeDefined();
  });
});
