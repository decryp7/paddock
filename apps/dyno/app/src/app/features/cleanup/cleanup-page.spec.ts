import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Notifier } from '../../shared/notifier';
import { CleanupStore } from '../../state/cleanup.store';
import { CleanupPage } from './cleanup-page';

const NOW = 1_700_000_000_000;

function render(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(CleanupPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock, store: TestBed.inject(CleanupStore) };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    await new Promise((r) => setTimeout(r));
    f.detectChanges();
  }
};
const text = (el: Element) => (el.textContent ?? '').replace(/\s+/g, ' ').trim();
const button = (root: ParentNode, label: string | RegExp) =>
  [...root.querySelectorAll<HTMLButtonElement>('button')].find((b) => (typeof label === 'string' ? text(b) === label || b.getAttribute('aria-label') === label : label.test(text(b)) || label.test(b.getAttribute('aria-label') ?? '')));
const dialog = () => document.querySelector('dy-cleanup-confirm-dialog')!;

async function scanned(fail: string[] = []) {
  const r = render(fail);
  await settle(r.f);
  button(r.el, /^Scan/)!.click();
  await settle(r.f);
  return r;
}

beforeEach(() => vi.spyOn(Date, 'now').mockReturnValue(NOW));
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = ''));
});

describe('dy-cleanup-page', () => {
  it('lists the categories by group with their risk and scans nothing until asked', async () => {
    const { f, el, mock } = render();
    const scan = vi.spyOn(mock, 'scanCleanup');
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Cleanup');
    expect(el.querySelectorAll('.group').length).toBeGreaterThan(3);
    expect(text(el.querySelector('.group')!)).toContain('System');
    expect(el.querySelector('.badge')?.textContent).toMatch(/Safe|Review|Personal files/);
    expect(scan).not.toHaveBeenCalled();
    expect(el.querySelector('[aria-label="Preview"]')).toBeNull();
    expect(text(el.querySelector('[aria-label="Quarantine"]')!)).toContain('Put back');
  });

  it('picking categories changes what the scan button says', async () => {
    const { f, el } = render();
    await settle(f);
    const before = text(button(el, /^Scan/)!);
    el.querySelector<HTMLInputElement>('.cat input:checked')!.click();
    await settle(f);
    expect(text(button(el, /^Scan/)!)).not.toBe(before);
  });

  it('scans, lists every item with its size and reason, and only then offers to clean', async () => {
    const { el, mock } = await scanned();
    const run = vi.spyOn(mock, 'runCleanup');
    const preview = el.querySelector('[aria-label="Preview"]')!;
    expect(preview.querySelectorAll('.line').length).toBeGreaterThan(5);
    expect(text(preview.querySelector('.line')!)).toMatch(/\d/);
    expect(text(preview)).toContain('selected');
    expect(run).not.toHaveBeenCalled();
  });

  it('asks for confirmation in a dialog, and cancelling deletes nothing', async () => {
    const { f, el, mock } = await scanned();
    const run = vi.spyOn(mock, 'runCleanup');
    button(el, /Clean selected/)!.click();
    await settle(f);
    expect(text(dialog())).toContain('Quarantine for 7 days');
    expect(text(dialog())).toContain('checked again right before it moves');
    button(dialog(), 'Cancel')!.click();
    await settle(f);
    expect(run).not.toHaveBeenCalled();
    expect(document.querySelector('dy-cleanup-confirm-dialog')).toBeNull();
  });

  it('confirming moves the selection to the quarantine and offers Undo, which puts it back', async () => {
    const { f, el, mock, store } = await scanned();
    const run = vi.spyOn(mock, 'runCleanup');
    button(el, /Clean selected/)!.click();
    await settle(f);
    button(dialog(), 'Move to quarantine')!.click();
    await settle(f);
    expect(run).toHaveBeenCalledWith(expect.any(String), expect.any(Array), 'quarantine');
    const result = el.querySelector('[aria-label="Result"]')!;
    expect(text(result)).toMatch(/Moved \d+ items? .* to the 7-day quarantine/);
    expect(store.scan()).toBeNull();
    expect(el.querySelector('[aria-label="Preview"]')).toBeNull();
    button(result, 'Undo')!.click();
    await settle(f);
    expect(el.querySelector('[aria-label="Result"]')).toBeNull();
  });

  it('can choose the Recycle Bin in the dialog, which has no Undo here', async () => {
    const { f, el, mock } = await scanned();
    const run = vi.spyOn(mock, 'runCleanup');
    button(el, /Clean selected/)!.click();
    await settle(f);
    button(dialog(), 'Recycle Bin / Trash')!.click();
    await settle(f);
    expect(text(dialog())).toContain('empty it');
    button(dialog(), 'Move to Recycle Bin / Trash')!.click();
    await settle(f);
    expect(run).toHaveBeenCalledWith(expect.any(String), expect.any(Array), 'recycleBin');
    const result = el.querySelector('[aria-label="Result"]')!;
    expect(text(result)).toContain('Recycle Bin');
    expect(button(result, 'Undo')).toBeUndefined();
  });

  it('clearing the selection disables cleaning; a category checkbox selects its items again', async () => {
    const { f, el } = await scanned();
    button(el, 'Clear selection')!.click();
    await settle(f);
    expect(button(el, /Clean selected/)!.disabled).toBe(true);
    el.querySelector<HTMLInputElement>('.cat-head input')!.click();
    await settle(f);
    expect(button(el, /Clean selected/)!.disabled).toBe(false);
  });

  it('refuses to open the dialog for a scan older than an hour and offers to scan again', async () => {
    const { f, el, store } = await scanned();
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
    const again = TestBed.createComponent(CleanupPage);
    again.detectChanges();
    vi.spyOn(Date, 'now').mockReturnValue(NOW + 61 * 60 * 1000);
    vi.advanceTimersByTime(31_000);
    again.detectChanges();
    expect(text(again.nativeElement)).toContain('more than an hour old');
    expect(button(again.nativeElement, /Scan again/)).toBeDefined();
    expect(button(again.nativeElement, /Clean selected/)).toBeUndefined();
    again.destroy();
    vi.useRealTimers();
    // The click path checks again even if the note has not appeared yet.
    button(el, /Clean selected/)!.click();
    await settle(f);
    expect(document.querySelector('dy-cleanup-confirm-dialog')).toBeNull();
    expect(TestBed.inject(Notifier).toasts().at(-1)!.text).toContain('more than an hour old');
    expect(store.scan()).not.toBeNull();
  });

  it('shows a failed category list with Retry', async () => {
    const { f, el } = render(['cleanupCategories']);
    await settle(f);
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
    expect(button(el, /Retry/)).toBeDefined();
  });

  it('shows a failed scan inline with Retry', async () => {
    const { f, el, mock } = render();
    await settle(f);
    vi.spyOn(mock, 'scanCleanup').mockRejectedValueOnce({ code: 'timeout', message: 'Took too long' });
    button(el, /^Scan/)!.click();
    await settle(f);
    expect(text(el.querySelector('.note.bad')!)).toContain('Took too long');
    button(el.querySelector('.note.bad')!, /Retry/)!.click();
    await settle(f);
    expect(el.querySelector('[aria-label="Preview"]')).not.toBeNull();
  });

  it('lists skipped items with the reason and can dismiss the result', async () => {
    const { f, el, mock } = await scanned();
    vi.spyOn(mock, 'runCleanup').mockResolvedValue({ freedBytes: 10, deleted: 1, failed: [{ path: '/t/busy.db', reason: 'In use by Example. Close it and try again.' }], quarantineId: null });
    button(el, /Clean selected/)!.click();
    await settle(f);
    button(dialog(), 'Move to quarantine')!.click();
    await settle(f);
    expect(text(el.querySelector('.failed')!)).toContain('In use by Example');
    button(el, 'Dismiss the result')!.click();
    await settle(f);
    expect(el.querySelector('[aria-label="Result"]')).toBeNull();
  });

  it('puts a waiting quarantine back from the list', async () => {
    const { f, el, mock } = render();
    const restore = vi.spyOn(mock, 'restoreQuarantine');
    await settle(f);
    button(el.querySelector('[aria-label="Quarantine"]')!, /Put back/)!.click();
    await settle(f);
    expect(restore).toHaveBeenCalledWith('q-1');
  });

  it('scrolls a long list in a virtualised viewport instead of rendering every row', async () => {
    const { f, el, mock } = render();
    const items = Array.from({ length: 500 }, (_, i) => ({ id: `big:${i}`, path: `/t/f${i}`, bytes: 10, modified: 0, folder: false, reason: 'Old', selected: true }));
    vi.spyOn(mock, 'scanCleanup').mockResolvedValue({
      id: 'scan-big',
      at: NOW,
      totalBytes: 5000,
      results: [{ category: 'system-temp', bytes: 5000, itemCount: 500, items, truncated: true, note: null }],
    });
    await settle(f);
    button(el, /^Scan/)!.click();
    await settle(f);
    expect(el.querySelector('cdk-virtual-scroll-viewport')).not.toBeNull();
    expect(el.querySelectorAll('.line').length).toBeLessThan(500);
    expect(text(el.querySelector('[aria-label="Preview"]')!)).toContain('Showing the 500 biggest of 500');
  });
});
