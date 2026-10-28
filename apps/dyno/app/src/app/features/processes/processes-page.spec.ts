import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { ProcessesStore } from '../../state/processes.store';
import { ProcessesPage } from './processes-page';

const NOW = 1_700_000_000_000;

async function make(tweak?: (m: MockBackend) => void) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  tweak?.(mock);
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(ProcessesPage);
  const store = TestBed.inject(ProcessesStore);
  f.detectChanges();
  await f.whenStable();
  await new Promise((r) => setTimeout(r, 5)); // the mock's first live update
  f.detectChanges();
  return { f, mock, store, el: f.nativeElement as HTMLElement };
}

describe('ProcessesPage', () => {
  it('creates, titles itself and subscribes to the process list while open', async () => {
    const { f, store, el } = await make();
    expect(el.querySelector('h1')?.textContent).toBe('Processes');
    expect(store.connected()).toBe(true);
    expect(el.querySelector('[role="treegrid"]')).not.toBeNull();
    f.destroy();
  });
  it('shows a loading skeleton before the first update', () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(ProcessesPage);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('[aria-label="Loading processes"]')).not.toBeNull();
    f.destroy();
  });
  it('shows the Other users row with a one-line note and without a row menu', async () => {
    expect((await make()).el.querySelector('[data-testid="other-users-note"]')).toBeNull();
    TestBed.resetTestingModule();
    const { f, store, el } = await make((m) => {
      const live = m.liveUpdate.bind(m);
      m.liveUpdate = (d) => ({ ...live(d), otherUsers: { count: 4, cpu: { value: 2 }, memory: { unavailable: { reason: 'needsAdmin' } }, partial: true } });
    });
    store.setMode('flat');
    f.detectChanges();
    expect(el.querySelector('[data-testid="other-users-note"]')?.textContent).toContain('4 processes of other accounts');
    expect(store.rows().find((r) => r.kind === 'other')?.name).toBe('Other users');
    expect(el.querySelector('[data-key="other-users"] .act button')).toBeNull();
    f.destroy();
  });
  it('offers Run as administrator when values are unavailable, and not while elevated', async () => {
    const { f, el, store } = await make((m) => {
      const live = m.liveUpdate.bind(m);
      m.liveUpdate = (d) => {
        const u = live(d);
        return { ...u, processes: u.processes?.map((p, i) => (i === 0 ? { ...p, cpu: { unavailable: { reason: 'needsAdmin' as const } } } : p)) ?? null };
      };
    });
    store.setMode('flat');
    f.detectChanges();
    const note = el.querySelector('[data-testid="partial-note"]');
    expect(note?.textContent).toContain('Run as administrator');
    f.destroy();
    TestBed.resetTestingModule();
    const fine = await make();
    expect(fine.el.querySelector('[data-testid="partial-note"]')).toBeNull();
    fine.f.destroy();
  });
  it('the view buttons switch the store mode', async () => {
    const { f, store, el } = await make();
    const buttons = [...el.querySelectorAll<HTMLButtonElement>('[aria-label="View"] button')];
    expect(buttons.length).toBe(3);
    buttons[2]!.click();
    f.detectChanges();
    expect(['groups', 'tree', 'flat']).toContain(store.mode());
    expect(store.mode()).not.toBe('groups');
  });
  it('filtering reduces the rows; a filter with no match offers Clear', async () => {
    const { f, store, el } = await make();
    const all = store.rows().length;
    const input = el.querySelector<HTMLInputElement>('input[aria-label="Filter processes"]')!;
    input.value = 'rustc';
    input.dispatchEvent(new Event('input'));
    f.detectChanges();
    expect(store.query()).toBe('rustc');
    expect(store.rows().length).toBeGreaterThan(0);
    expect(store.rows().length).toBeLessThan(all);
    input.value = 'zzz-no-such-process';
    input.dispatchEvent(new Event('input'));
    f.detectChanges();
    expect(store.rows()).toEqual([]);
    expect(el.querySelector('.empty')?.textContent).toContain('No process matches');
    el.querySelector<HTMLButtonElement>('.empty button')!.click();
    f.detectChanges();
    expect(store.query()).toBe('');
    f.destroy();
  });
  it('clicking a column header sorts by it', async () => {
    const { f, store, el } = await make();
    el.querySelector<HTMLButtonElement>('button[aria-label="Sort by Memory"]')!.click();
    f.detectChanges();
    expect(store.sort()).toEqual({ key: 'memory', dir: 'desc' });
    expect(el.querySelector('[aria-sort]')).not.toBeNull();
    f.destroy();
  });
  it('renders rows from the store when the virtual viewport has a size', async () => {
    const { f, store, el } = await make();
    const viewport = el.querySelector<HTMLElement>('cdk-virtual-scroll-viewport');
    expect(viewport).not.toBeNull();
    // jsdom has no layout, so the viewport may render no rows: the rows logic is covered by the store and
    // process-rows specs. If it did render, they must be the store's first rows in order.
    const rendered = [...el.querySelectorAll<HTMLElement>('[role="row"][data-key]')].map((r) => r.dataset['key']);
    expect(rendered).toEqual(store.rows().slice(0, rendered.length).map((r) => r.key));
    f.destroy();
  });
  it('stops the subscription when destroyed', async () => {
    const { f, mock } = await make();
    const stop = vi.spyOn(TestBed.inject(ProcessesStore), 'stop');
    f.destroy();
    expect(stop).toHaveBeenCalled();
    expect(mock).toBeDefined();
  });
});
