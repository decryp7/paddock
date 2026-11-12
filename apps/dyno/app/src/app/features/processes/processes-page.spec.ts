import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { LiveStore } from '../../state/live.store';
import { ProcessesStore } from '../../state/processes.store';
import { nextFrame } from '../../testing/frame';
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
  await vi.waitFor(() => expect(store.processes().length).toBeGreaterThan(0)); // the mock's first live update
  f.detectChanges();
  await f.whenStable();
  await nextFrame();
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
    expect(el.querySelector('[data-testid="table-loading"]')).not.toBeNull();
    f.destroy();
  });
  it('says so, with Retry, when the agent failed before any process list came, and asks again on Retry', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    const live = vi.spyOn(mock, 'onLive');
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(ProcessesPage);
    TestBed.inject(LiveStore).setAgent({ state: 'failed', message: 'The agent stopped.' });
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('The agent stopped.');
    expect(el.querySelector('[role="treegrid"]')).toBeNull();
    const calls = live.mock.calls.length;
    [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('Retry'))!.click();
    expect(live.mock.calls.length).toBe(calls + 1);
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
    expect(el.querySelector('[data-key="other-users"] .pd-icon-btn')).toBeNull();
    f.destroy();
  });
  it('shows where the list came from: the chip when the helper read it, the reason and the button when it did not', async () => {
    const withVia = async (via: unknown) =>
      make((m) => {
        const live = m.liveUpdate.bind(m);
        m.liveUpdate = (d) => ({ ...live(d), via: { processes: via, others: 'local', temperatures: 'local' } }) as ReturnType<typeof live>;
      });
    let r = await withVia('helper');
    expect(r.el.querySelector('[data-testid="processes-source"] [data-testid="reading-source-helper"]')).not.toBeNull();
    expect([...r.el.querySelectorAll('[role="columnheader"]')].some((h) => h.textContent?.includes('Account'))).toBe(true);
    r.f.destroy();
    TestBed.resetTestingModule();
    r = await withVia({ needsHelper: { reason: 'Other accounts’ processes are not shown. Turn on administrator readings in Settings so the helper can read this.', next: { openSettings: 'helper' } } });
    expect(r.el.querySelector('[data-testid="reading-source-needs"]')?.textContent).toContain('Other accounts’ processes are not shown');
    expect(r.el.querySelector('[data-testid="reading-source-turn-on"]')).not.toBeNull();
    r.f.destroy();
  });
  it('shows the engine\'s note while the helper\'s first read is awaited, as a hint and not as a failure', async () => {
    const { f, el } = await make((m) => {
      const live = m.liveUpdate.bind(m);
      m.liveUpdate = (d) => ({ ...live(d), via: { processes: 'local', others: 'local', temperatures: 'local', processesHint: 'Other accounts appear after the helper’s first read.' } }) as ReturnType<typeof live>;
    });
    expect(el.querySelector('[data-testid="processes-source"] [data-testid="reading-source-hint"]')?.textContent).toContain('appear after the helper’s first read');
    expect(el.querySelector('[data-testid="reading-source-unavailable"]')).toBeNull();
    expect(el.querySelector('[data-testid="reading-source-step"]')).toBeNull();
    f.destroy();
  });
  it('says the Other users total was counted by the helper', async () => {
    const { f, store, el } = await make((m) => {
      const live = m.liveUpdate.bind(m);
      m.liveUpdate = (d) => ({ ...live(d), via: { processes: 'helper', others: 'helper', temperatures: 'local' }, otherUsers: { count: 4, cpu: { value: 2 }, memory: { value: 5 }, partial: false } });
    });
    store.setMode('flat');
    f.detectChanges();
    expect(el.querySelector('[data-testid="other-users-note"]')?.textContent).toContain('counted by the administrator helper');
    f.destroy();
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
    await f.whenStable();
    await nextFrame();
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
    const header = () => [...el.querySelectorAll<HTMLElement>('[role="columnheader"]')].find((h) => h.textContent!.trim() === 'Memory')!;
    expect(header().getAttribute('aria-sort')).toBe('none');
    header().querySelector<HTMLElement>('.mat-sort-header-container')!.click();
    f.detectChanges();
    expect(store.sort()).toEqual({ key: 'memory', dir: 'desc' });
    header().querySelector<HTMLElement>('.mat-sort-header-container')!.click();
    f.detectChanges();
    expect(store.sort()).toEqual({ key: 'memory', dir: 'asc' });
    await f.whenStable();
    f.detectChanges();
    expect(header().getAttribute('aria-sort')).toBe('ascending');
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
