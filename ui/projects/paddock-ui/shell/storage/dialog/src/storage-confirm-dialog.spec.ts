import { TestBed } from '@angular/core/testing';
import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { PdStorageConfirmDialog } from './storage-confirm-dialog';

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();
const open = (inputs: { what?: string; bytes?: number; data?: boolean } = {}) => renderFramed(PdStorageConfirmDialog, { id: 'pd.storage.confirm', inputs });

describe('the storage confirm dialog', () => {
  it('names what goes and how much; Keep has the focus, Delete data is the destructive action for the user’s data', async () => {
    const d = await open({ what: 'Logs', bytes: 5_000_000, data: true });
    // The title says what; the body gives the facts and does not say it again.
    expect(text(d.q('storage-confirm-text')!)).toBe('Up to 4.8 MB is removed.');
    expect(d.title()).toBe('Clear logs?');
    expect(text(d.q('storage-confirm-data')!)).toContain('your own data');
    expect(d.action('clear')!.textContent!.trim()).toBe('Delete data');
    expect(d.action('clear')!.className).toContain('pd-frame-btn--danger');
    expect(document.activeElement).toBe(d.action('keep'));
  });

  it('says the cache is rebuilt, and answers true for Clear cache and is dismissed by Keep', async () => {
    const a = await open({ what: 'the cache', bytes: 10, data: false });
    expect(a.q('storage-confirm-data')).toBeNull();
    expect(text(a.el)).toContain('rebuilds it');
    expect(a.action('clear')!.textContent!.trim()).toBe('Clear cache');
    await a.press('clear');
    expect(a.outcome()).toEqual({ kind: 'answered', value: true });
    TestBed.resetTestingModule();
    const b = await open({ what: 'the cache', bytes: 10 });
    await b.press('keep');
    expect(b.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('opened without anything to clear it says so inline and offers only Close', async () => {
    const d = await open();
    expect(d.q('storage-confirm-invalid')?.getAttribute('role')).toBe('alert');
    expect(d.actions()).toEqual(['close']);
    await d.press('close');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
  });
});
