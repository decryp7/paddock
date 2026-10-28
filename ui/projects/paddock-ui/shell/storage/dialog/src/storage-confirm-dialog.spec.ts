import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import { PdStorageConfirmDialog } from './storage-confirm-dialog';

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

describe('the storage confirmation', () => {
  async function open(inputs: { what?: string; bytes?: number; data?: boolean } = {}) {
    const closed: unknown[] = [];
    const titles: string[] = [];
    const ref: DialogRef<boolean> = { closed: Promise.resolve(undefined), close: (r) => void closed.push(r), setTitle: (t) => void titles.push(t) };
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: DIALOG_REF, useValue: ref }] });
    const fixture = TestBed.createComponent(PdStorageConfirmDialog);
    if (inputs.what !== undefined) fixture.componentRef.setInput('what', inputs.what);
    if (inputs.bytes !== undefined) fixture.componentRef.setInput('bytes', inputs.bytes);
    if (inputs.data !== undefined) fixture.componentRef.setInput('data', inputs.data);
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    const q = (id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);
    return { el, q, closed, titles };
  }

  it('names what goes and how much, and keeps the focus on Keep, never on Clear', async () => {
    const d = await open({ what: 'Logs', bytes: 5_000_000, data: true });
    // The title says what; the body gives the facts and does not say it again.
    expect(text(d.q('storage-confirm-text')!)).toBe('Up to 5 MB is removed.');
    expect(d.titles.at(-1)).toBe('Clear logs?');
    expect(text(d.q('storage-confirm-data')!)).toContain('your own data');
    const keep = d.q('storage-confirm-keep')!;
    expect(keep.hasAttribute('data-default')).toBe(true);
    expect(keep.hasAttribute('data-dialog-close')).toBe(true);
    expect(d.q('storage-confirm-yes')!.hasAttribute('data-default')).toBe(false);
    expect(d.q('storage-confirm-yes')!.className).toContain('pd-sc__btn--danger');
  });

  it('says the cache is rebuilt, and resolves true for Clear and false for Keep', async () => {
    const a = await open({ what: 'the cache', bytes: 10, data: false });
    expect(a.q('storage-confirm-data')).toBeNull();
    expect(text(a.el)).toContain('rebuilds it');
    a.q('storage-confirm-yes')!.click();
    expect(a.closed).toEqual([true]);
    TestBed.resetTestingModule();
    const b = await open({ what: 'the cache', bytes: 10 });
    b.q('storage-confirm-keep')!.click();
    expect(b.closed).toEqual([false]);
  });

  it('opened without anything to clear it says so inline and offers no Clear', async () => {
    const d = await open();
    expect(d.q('storage-confirm-invalid')?.getAttribute('role')).toBe('alert');
    expect(d.q('storage-confirm-yes')).toBeNull();
    expect(d.titles).toEqual([]);
    d.q('storage-confirm-close')!.click();
    expect(d.closed).toEqual([undefined]);
  });
});
