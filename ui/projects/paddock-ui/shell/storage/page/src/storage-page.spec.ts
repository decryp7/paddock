import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { MemoryStorageBackend, STORAGE_CONFIRM_DIALOG_ID, provideShellStorage, sampleCategories, type MemoryStorageOptions } from '@decryp7/paddock-ui/shell/storage';
import { PdStoragePage } from './storage-page';

async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

/** The dialog service as the page uses it: records what was opened and answers like the confirmation window. */
class FakeDialogs {
  readonly opened: { id: string; inputs: Record<string, unknown> }[] = [];
  answer: boolean | undefined = true;
  open(id: string, options: { inputs?: Record<string, unknown> } = {}) {
    this.opened.push({ id, inputs: options.inputs ?? {} });
    return { closed: Promise.resolve(this.answer), close: () => undefined };
  }
}

function held(options: MemoryStorageOptions = {}) {
  const queue: (() => void)[] = [];
  const memory = new MemoryStorageBackend({ schedule: (step) => queue.push(step), ...options });
  return {
    memory,
    pending: () => queue.length,
    async next(): Promise<void> {
      queue.shift()?.();
      await settle();
    },
    async all(): Promise<void> {
      while (queue.length) queue.shift()?.();
      await settle();
    },
  };
}

describe('the Storage page', () => {
  async function open(options: MemoryStorageOptions = { measured: true }) {
    const h = held(options);
    const dialogs = new FakeDialogs();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellStorage({ backend: () => h.memory }), { provide: DialogService, useValue: dialogs }] });
    const fixture = TestBed.createComponent(PdStoragePage);
    const el = fixture.nativeElement as HTMLElement;
    const flush = async (): Promise<void> => {
      await settle();
      await fixture.whenStable();
      await settle();
      await fixture.whenStable();
    };
    await flush();
    const q = (id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);
    const click = async (id: string): Promise<void> => {
      q(id)!.click();
      await flush();
    };
    return { h, dialogs, el, q, click, flush };
  }

  it('lists what the app keeps with sizes, marks lower bounds and offers Clear only where something can go', async () => {
    const p = await open();
    expect(text(p.q('storage-total')!)).toContain('≥');
    expect(p.el.querySelectorAll('tbody tr')).toHaveLength(3);
    expect(text(p.q('storage-row-cache')!)).toContain('Cache');
    expect(text(p.q('storage-row-models')!)).toContain('≥');
    expect(text(p.q('storage-clear-cache')!)).toContain('Clear cache');
    expect(p.q('storage-clear-cache')!.className).toContain('pd-storage__btn--primary');
    expect(p.q('storage-clear-logs')).not.toBeNull();
    expect(p.q('storage-clear-cache-row')).toBeNull();
    expect(p.el.querySelector('caption')?.textContent).toContain('What the app keeps');
    for (const b of Array.from(p.el.querySelectorAll('.pd-storage__actions button'))) expect(b.getAttribute('aria-label')).toBeTruthy();
  });

  it('asks first, then clears the cache and says what it freed', async () => {
    const p = await open();
    await p.click('storage-clear-cache');
    expect(p.dialogs.opened).toEqual([{ id: STORAGE_CONFIRM_DIALOG_ID, inputs: { what: 'Cache', bytes: 48_000_000, data: false } }]);
    expect(p.q('storage-working')).not.toBeNull();
    expect(p.q('storage-still-working')?.textContent).toContain('Still working');
    await p.h.next();
    await p.flush();
    expect(text(p.q('storage-working')!)).toContain('Removing');
    expect(p.q('storage-cancel')).not.toBeNull();
    await p.h.all();
    await p.flush();
    expect(text(p.q('storage-result')!)).toContain('freed');
    await p.h.all();
    await p.flush();
    expect(p.q('storage-working')).toBeNull();
    expect(p.q('storage-clear-cache')!.hasAttribute('disabled')).toBe(true);
  });

  it('does nothing when the user keeps everything, and names the data when it is the users own', async () => {
    const p = await open();
    p.dialogs.answer = false;
    await p.click('storage-clear-cache');
    expect(p.h.pending()).toBe(0);
    p.dialogs.answer = true;
    await p.click('storage-clear-logs');
    expect(p.dialogs.opened.at(-1)).toEqual({ id: STORAGE_CONFIRM_DIALOG_ID, inputs: { what: 'Logs', bytes: 5_000_000, data: true } });
    await p.h.all();
    await p.flush();
    expect(text(p.q('storage-result')!)).toContain('freed');
  });

  it('shows Cancel while it runs and what was done when it stopped', async () => {
    const p = await open();
    await p.click('storage-measure');
    expect(p.q('storage-working')).not.toBeNull();
    await p.click('storage-cancel');
    expect(text(p.q('storage-cancel')!)).toBe('Cancelling…');
    expect(p.q('storage-cancel')!.hasAttribute('disabled')).toBe(true);
    await p.h.all();
    await p.flush();
    expect(p.q('storage-working')).toBeNull();
    await p.click('storage-clear-logs');
    await p.h.next();
    await p.click('storage-cancel');
    await p.h.all();
    await p.flush();
    expect(text(p.q('storage-result')!)).toContain('Stopped');
  });

  it('shows a refusal inline with Retry, and a failed ask with Retry', async () => {
    const p = await open();
    p.h.memory.reveal = () => Promise.reject({ code: 'notFound', message: 'That folder is gone.', retryable: false, hint: 'Measure again.' });
    await p.click('storage-show-cache');
    const failure = p.q('storage-failure')!;
    expect(failure.getAttribute('role')).toBe('alert');
    expect(text(failure)).toContain('That folder is gone.');
    expect(text(failure)).toContain('Measure again.');
    expect(failure.querySelector('[data-testid="storage-retry"]')).toBeNull();
    (failure.querySelector('button') as HTMLButtonElement).click();
    await p.flush();
    expect(p.q('storage-failure')).toBeNull();

    TestBed.resetTestingModule();
    const r = await (async () => {
      const h = held({});
      h.memory.last = () => Promise.reject(new Error('the engine is down'));
      TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellStorage({ backend: () => h.memory }), { provide: DialogService, useValue: new FakeDialogs() }] });
      const f = TestBed.createComponent(PdStoragePage);
      await settle();
      await f.whenStable();
      return f.nativeElement as HTMLElement;
    })();
    expect(r.querySelector('[data-testid="storage-error"]')?.getAttribute('role')).toBe('alert');
    expect(r.querySelector('[data-testid="storage-load-retry"]')).not.toBeNull();
  });

  it('lists what a clear left in place and why', async () => {
    const categories = sampleCategories();
    const p = await open({ measured: true, categories });
    p.h.memory.send({
      state: 'cleared',
      result: { freedBytes: 0, cancelled: false, categories: [{ id: 'logs', freedBytes: 0, removedFiles: 0, skippedCount: 2, skipped: [{ path: '/demo/app/logs/a.log', reason: 'in use' }], error: 'It holds your own data: confirm to clear it.' }] },
    });
    await p.flush();
    expect(text(p.q('storage-result-error')!)).toContain('confirm');
    expect(text(p.q('storage-skipped')!)).toContain('2 items left in place');
    expect(text(p.q('storage-skipped')!)).toContain('in use');
  });

  it('shows the size tree of a folder as a grouped table, and hides it again', async () => {
    const p = await open();
    await p.click('storage-tree-cache');
    expect(text(p.q('storage-working')!)).toContain('Measuring what the app keeps');
    await p.h.next();
    await p.flush();
    expect(text(p.q('storage-working')!)).toContain('Measuring Cache');
    await p.h.all();
    await p.flush();
    const tree = p.q('storage-tree')!;
    expect(text(tree.querySelector('h3')!)).toBe('What is in Cache');
    expect(tree.querySelector('pd-table')).not.toBeNull();
    expect(text(tree)).toContain('thumbnails');
    expect(text(tree)).toContain('7 smaller items');
    expect(p.q('storage-tree-cache')!.getAttribute('aria-expanded')).toBe('true');
    await p.click('storage-tree-cache');
    expect(p.q('storage-tree')).toBeNull();
    await p.click('storage-tree-cache');
    expect(p.q('storage-tree')).not.toBeNull();
  });

  it('says when the walk stopped early', async () => {
    const h = held({ measured: true });
    const own = h.memory;
    const dialogs = new FakeDialogs();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellStorage({ backend: () => own }), { provide: DialogService, useValue: dialogs }] });
    const fixture = TestBed.createComponent(PdStoragePage);
    await settle();
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    (el.querySelector('[data-testid="storage-tree-models"]') as HTMLButtonElement).click();
    await settle();
    own.send({ state: 'tree', tree: { id: 'models', capped: true, root: { name: 'Models', bytes: 5, folded: 0, children: [] } } });
    await settle();
    await fixture.whenStable();
    expect(el.querySelector('[data-testid="storage-tree-capped"]')?.textContent).toContain('stopped early');
  });
});
