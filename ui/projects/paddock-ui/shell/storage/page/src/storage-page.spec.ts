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
    /** A category's row of the table. */
    const row = (id: string): HTMLElement => el.querySelector(`.pd-row[data-key="${id}"]`) as HTMLElement;
    /** Presses a row's hover icon button. */
    const inline = async (id: string, label: string): Promise<void> => {
      (row(id).querySelector(`.pd-inline[aria-label="${label}"]`) as HTMLButtonElement).click();
      await flush();
    };
    /** Opens a row's "…" menu and chooses an item. */
    const menu = async (id: string, label: string): Promise<void> => {
      (row(id).querySelector('.pd-more') as HTMLButtonElement).click();
      await flush();
      const item = Array.from(document.querySelectorAll<HTMLElement>('.pd-popmenu__item')).find((i) => text(i) === label);
      item!.click();
      await flush();
    };
    const menuItems = async (id: string): Promise<{ label: string; danger: boolean }[]> => {
      (row(id).querySelector('.pd-more') as HTMLButtonElement).click();
      await flush();
      const items = Array.from(document.querySelectorAll<HTMLElement>('.pd-popmenu__item')).map((i) => ({ label: text(i), danger: i.classList.contains('pd-popmenu__item--danger') }));
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      await flush();
      return items;
    };
    return { h, dialogs, el, q, click, flush, row, inline, menu, menuItems };
  }

  it('lists what the app keeps in one shared table, one line per row, grouped by what it is, with lower bounds written in words', async () => {
    const p = await open();
    expect(text(p.q('storage-total')!)).toContain('at least');
    expect(text(p.q('storage-total')!)).not.toContain('≥');
    expect(p.el.querySelector('pd-table[data-testid="storage-table"]')).not.toBeNull();
    expect(Array.from(p.el.querySelectorAll('.pd-group')).map((g) => text(g.querySelector('.pd-group-name')!))).toEqual(['Cache: rebuilt by the app', 'Your data: cannot be rebuilt']);
    // The group heading carries the aggregate, right-aligned in the Size column, rounded down for a lower bound.
    expect(text(p.el.querySelector('.pd-group-summary.pd-end')!)).toBe('46 MB');
    expect(text(Array.from(p.el.querySelectorAll('.pd-group-summary.pd-end'))[1]!)).toMatch(/^≥ \d/);
    expect(text(p.row('models'))).toContain('≥');
    expect(p.row('cache').querySelectorAll('.pd-storage__title')).toHaveLength(1);
    expect(text(p.row('cache'))).not.toContain('CacheCache');
    expect(p.el.querySelector('.pd-th.pd-end')).not.toBeNull();
    // The cache button is an ordinary button, not the page's accent primary.
    expect(p.q('storage-clear-cache')!.className).not.toContain('primary');
    expect(text(p.q('storage-clear-cache')!)).toContain('Clear cache');
  });

  it('gives a row two hover icon buttons and a menu that ends with Clear in danger text, only where something can go', async () => {
    const p = await open();
    expect(Array.from(p.row('logs').querySelectorAll('.pd-inline')).map((b) => b.getAttribute('aria-label'))).toEqual(['What is inside', 'Show in the file manager']);
    expect(await p.menuItems('logs')).toEqual([
      { label: 'What is inside', danger: false },
      { label: 'Show in the file manager', danger: false },
      { label: 'Clear logs', danger: true },
    ]);
    TestBed.resetTestingModule();
    const gone = await open({ measured: true, categories: sampleCategories().map((c) => (c.id === 'models' ? { ...c, clearableItems: 0, exists: false } : c)) });
    expect(gone.row('models').querySelectorAll('.pd-inline')).toHaveLength(0);
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
    await p.menu('logs', 'Clear logs');
    expect(p.dialogs.opened.at(-1)).toEqual({ id: STORAGE_CONFIRM_DIALOG_ID, inputs: { what: 'Logs', bytes: 5_000_000, data: true } });
    await p.h.all();
    await p.flush();
    expect(text(p.q('storage-result')!)).toContain('freed');
    // A cache category clears from its own row too, without the data confirmation.
    TestBed.resetTestingModule();
    const q = await open();
    await q.menu('cache', 'Clear cache');
    expect(q.dialogs.opened.at(-1)?.inputs).toMatchObject({ data: false });
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
    await p.menu('logs', 'Clear logs');
    await p.h.next();
    await p.click('storage-cancel');
    await p.h.all();
    await p.flush();
    expect(text(p.q('storage-result')!)).toContain('Stopped');
  });

  it('shows a refusal inline with its hint, and a failed ask with Retry', async () => {
    const p = await open();
    p.h.memory.reveal = () => Promise.reject({ code: 'notFound', message: 'That folder is gone.', retryable: false, hint: 'Measure again.' });
    await p.inline('cache', 'Show in the file manager');
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
    const p = await open({ measured: true, categories: sampleCategories() });
    p.h.memory.send({
      state: 'cleared',
      result: { freedBytes: 0, cancelled: false, categories: [{ id: 'logs', freedBytes: 0, removedFiles: 0, skippedCount: 2, skipped: [{ path: '/demo/app/logs/a.log', reason: 'in use' }], error: 'It holds your own data: confirm to clear it.' }] },
    });
    await p.flush();
    expect(text(p.q('storage-result-error')!)).toContain('confirm');
    expect(text(p.q('storage-skipped')!)).toContain('2 items left in place');
    expect(text(p.q('storage-skipped')!)).toContain('in use');
  });

  it('shows the size tree with the folder size on its group row and glyphs on the items, and closes it again', async () => {
    const p = await open();
    await p.inline('cache', 'What is inside');
    expect(text(p.q('storage-working')!)).toContain('Measuring what the app keeps');
    await p.h.next();
    await p.flush();
    expect(text(p.q('storage-working')!)).toContain('Measuring Cache');
    await p.h.all();
    await p.flush();
    const tree = p.q('storage-tree')!;
    expect(text(tree.querySelector('h3')!)).toBe('What is in Cache');
    const groups = Array.from(tree.querySelectorAll('.pd-group'));
    expect(groups.map((g) => text(g.querySelector('.pd-group-name')!))).toEqual(['thumbnails', 'previews', 'Other items']);
    expect(groups[0]?.querySelector('.pd-group-name')?.classList.contains('pd-mono')).toBe(true);
    // "Other items" is a label, not a folder name: not mono, and its fold row has no kind glyph.
    expect(groups[2]?.querySelector('.pd-group-name')?.classList.contains('pd-mono')).toBe(false);
    expect(tree.querySelectorAll('.pd-storage__nokind').length).toBe(1);
    expect(text(p.el.querySelector('pd-table[data-testid="storage-table"]')!)).toContain('3 kinds of data');
    // The folder's size and share are on its heading; its two items below add up to it.
    expect(text(groups[0]!.querySelector('.pd-group-summary.pd-end')!)).toMatch(/MB/);
    expect(text(tree)).toContain('7 smaller items');
    expect(text(tree)).toMatch(/\d+ items/);
    expect(tree.querySelectorAll('.pd-storage__item pd-glyph svg').length).toBeGreaterThan(3);
    expect(tree.querySelector('[data-testid="storage-tree-capped"]')).toBeNull();
    p.q('storage-tree-close')!.click();
    await p.flush();
    expect(p.q('storage-tree')).toBeNull();
    await p.inline('cache', 'What is inside');
    expect(p.q('storage-tree')).not.toBeNull();
    await p.inline('cache', 'What is inside');
    expect(p.q('storage-tree')).toBeNull();
  });

  it('says when the walk stopped early', async () => {
    const p = await open();
    await p.inline('models', 'What is inside');
    p.h.memory.send({ state: 'tree', tree: { id: 'models', capped: true, root: { name: 'Models', bytes: 5, folded: 0, children: [] } } });
    await p.flush();
    expect(p.q('storage-tree-capped')?.textContent).toContain('stopped early');
  });
});
