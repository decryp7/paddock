import { splitInstance } from './events-grid';
import { ManualClock } from '../../testing/manual-clock';
import { TestBed } from '@angular/core/testing';
import { all, buttonByText, el, provideTinyBackend, settle, testLogger, text, tinyBackend } from '../../testing/helpers';
import { Page, PageRequest, RecordView } from '../core/api.types';
import { DebriefBackend } from '../core/backend';
import { MockBackend } from '../core/mock/mock-backend';
import { withMockFailures } from '../core/mock/mock-failures';
import { tinyDataset } from '../../testing/helpers';
import { LiveTail } from '../workspace/live-tail';
import { LiveFeed } from '../live/live-feed';
import { LiveFollow } from '../live/live-follow.store';
import { WorkspaceStore } from '../core/workspace.store';
import { SettingsStore } from '../settings/settings.store';
import { EventsGrid } from './events-grid';
import { GridSelection } from './grid-selection';
import { RecordActions } from '../context/record-actions';
import { HighlightRules } from '../projects/bookmarks/highlight-rules.store';
import { at } from '../../testing/must';

const VIEWPORT_H = 260;

/** A scroll by the user: a real one always comes with an input event (the grid tells it from the browser's own scroll adjustments by that). */
function userScroll(vp: HTMLElement): void {
  vp.dispatchEvent(new WheelEvent('wheel', { deltaY: 0, bubbles: true }));
  vp.dispatchEvent(new Event('scroll'));
}

/** Pages are fetched once scrolling has paused: waits on that condition (the store's idle timer is no longer pending), not on a fixed time. */
async function scrollIdle(f: { componentInstance: unknown; whenStable(): Promise<unknown> }): Promise<void> {
  const rowsOf = (f.componentInstance as { rows: { scrollIdlePending: boolean } }).rows;
  await settle(f as never); // the fill effect schedules the fetch
  await vi.waitFor(() => expect(rowsOf.scrollIdlePending).toBe(false));
  await settle(f as never);
}

function fakeRow(i: number): RecordView {
  return { id: i, ts: 1_700_000_000_000 + i, tsState: 'parsed', level: 'Information', message: `row ${i}`, template: null, exception: null, fields: { n: i }, source: 0, sourceName: 's.log', line: i + 1, byteOffset: 0, group: 'g', markers: [] };
}

/** Backend with a huge synthetic result to exercise paging and scroll compression. */
class HugeBackend extends MockBackend {
  requests: PageRequest[] = [];
  constructor(private readonly total: number) {
    super({ latency: false, dataset: () => ({ sources: [], unrecognized: [], records: [] }) });
  }
  override queryPage(req: PageRequest): Promise<Page> {
    this.requests.push(req);
    if (req.anchor != null) return Promise.resolve({ total: this.total, offset: req.anchor, rows: [fakeRow(req.anchor)], anchorIndex: req.anchor });
    const rows = Array.from({ length: Math.max(0, Math.min(req.limit, this.total - req.offset)) }, (_, k) => fakeRow(req.offset + k));
    return Promise.resolve({ total: this.total, offset: req.offset, rows, anchorIndex: null });
  }
}

describe('EventsGrid', () => {
  // Restored after each test (restoreMocks in vitest-base.config.mts).
  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
        return this.classList?.contains('viewport') ? VIEWPORT_H : 0;
    });
  });
  beforeEach(() => localStorage.clear());

  async function render(backend: DebriefBackend = tinyBackend()) {
    TestBed.configureTestingModule({ providers: [provideTinyBackend(backend)] });
    const fixture = TestBed.createComponent(EventsGrid);
    const store = TestBed.inject(WorkspaceStore);
    await settle(fixture);
    return { fixture, store };
  }

  const rows = (f: Awaited<ReturnType<typeof render>>['fixture']) => all(f, '.row:not(.placeholder)');

  it('renders the first page with totals', async () => {
    const { fixture, store } = await render();
    expect(rows(fixture)).toHaveLength(11);
    expect(el(fixture, '.total').textContent).toBe('11');
    expect(text(fixture)).toContain('of 11');
    expect(store.matchTotal()).toBe(11);
    expect(at(rows(fixture), 0).textContent).toContain('Application starting');
    expect(all(fixture, '.row debrief-marker-icon').length).toBeGreaterThan(0);
    // The event without a timestamp stays in the grid, marked in its time cell.
    const untimed = all(fixture, '.time.untimed');
    expect(untimed).toHaveLength(1);
    expect(at(untimed, 0).textContent?.trim()).toBe('no time');
  });

  it('follows the density and date format settings', async () => {
    const { fixture } = await render();
    const settings = TestBed.inject(SettingsStore);
    expect(fixture.nativeElement.style.getPropertyValue('--row-h')).toBe('24px');
    expect(at(rows(fixture), 0).querySelector('.time')?.textContent).toMatch(/^\d+ \w{3} /);
    settings.setDensity('compact');
    settings.setDateFormat('iso');
    await settle(fixture);
    expect(fixture.nativeElement.style.getPropertyValue('--row-h')).toBe('22px');
    expect(at(rows(fixture), 0).querySelector('.time')?.textContent).toMatch(/^\d{4}-\d{2}-\d{2} /);
  });

  it('selects rows by click and keyboard', async () => {
    const { fixture, store } = await render();
    at(rows(fixture), 2).click();
    await settle(fixture);
    expect(store.selected()?.id).toBe(2);
    expect(at(rows(fixture), 2).classList.contains('sel')).toBe(true);
    const vp = el(fixture, '.viewport');
    const key = (k: string) => vp.dispatchEvent(new KeyboardEvent('keydown', { key: k, cancelable: true }));
    key('ArrowDown');
    expect(store.selected()?.id).toBe(3);
    key('ArrowUp');
    key('ArrowUp');
    expect(store.selected()?.id).toBe(1);
    key('End');
    expect(store.selected()?.id).toBe(10);
    key('Home');
    expect(store.selected()?.id).toBe(0);
    key('PageDown');
    expect(store.selected()!.id).toBeGreaterThan(0);
    key('PageUp');
    expect(store.selected()?.id).toBe(0);
    key('x');
    expect(store.selected()?.id).toBe(0);
  });

  it('names the focused row with aria-activedescendant so screen readers announce it', async () => {
    const { fixture } = await render();
    const vp = el(fixture, '.viewport');
    expect(vp.hasAttribute('aria-activedescendant')).toBe(false);
    vp.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', cancelable: true }));
    await settle(fixture);
    const id = vp.getAttribute('aria-activedescendant');
    expect(id).toMatch(/^events-row-\d+-0$/);
    const row = fixture.nativeElement.querySelector(`#${id}`) as HTMLElement;
    expect(row.getAttribute('role')).toBe('row');
    expect(row.classList.contains('focused')).toBe(true);
    vp.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', cancelable: true }));
    await settle(fixture);
    expect(vp.getAttribute('aria-activedescendant')).toMatch(/-1$/);
    expect(all(fixture, '.row.focused')).toHaveLength(1);
  });

  describe('multi-select', () => {
    const click =(row: HTMLElement, init: MouseEventInit = {}) => row.dispatchEvent(new MouseEvent('click', { bubbles: true, ...init }));
    const selectedIndexes = (f: Awaited<ReturnType<typeof render>>['fixture']) =>
      rows(f)
        .map((r, i) => (r.getAttribute('aria-selected') === 'true' ? i : -1))
        .filter((i) => i >= 0);

    it('is announced as a multi-selectable grid', async () => {
      const { fixture } = await render();
      expect(el(fixture, '[role=grid]').getAttribute('aria-multiselectable')).toBe('true');
      expect(rows(fixture).every((r) => r.getAttribute('aria-selected') === 'false')).toBe(true);
    });

    it('shift+click selects a range, ctrl/cmd+click toggles, a plain click selects one row', async () => {
      const { fixture, store } = await render();
      click(at(rows(fixture), 2));
      await settle(fixture);
      click(at(rows(fixture), 5), { shiftKey: true });
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([2, 3, 4, 5]);
      expect(store.selected()?.id).toBe(5);
      click(at(rows(fixture), 0), { shiftKey: true });
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([0, 1, 2]);
      click(at(rows(fixture), 1), { ctrlKey: true });
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([0, 2]);
      expect(store.selected()?.id).toBe(1);
      click(at(rows(fixture), 8), { metaKey: true });
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([0, 2, 8]);
      click(at(rows(fixture), 6));
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([6]);
      expect(at(rows(fixture), 6).classList.contains('sel')).toBe(true);
    });

    it('prevents text selection only for modified clicks', async () => {
      const { fixture } = await render();
      const down = (init: MouseEventInit) => {
        const e = new MouseEvent('mousedown', { bubbles: true, cancelable: true, ...init });
        at(rows(fixture), 1).dispatchEvent(e);
        return e.defaultPrevented;
      };
      expect(down({})).toBe(false);
      expect(down({ shiftKey: true })).toBe(true);
      expect(down({ ctrlKey: true })).toBe(true);
      expect(down({ metaKey: true })).toBe(true);
    });

    it('a row is draggable except from its message cell, which stays selectable text (WebKit)', async () => {
      const { fixture } = await render();
      const row = at(rows(fixture), 2);
      const press = (el: Element) => el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
      press(row.querySelector('.msg')!);
      expect(row.draggable).toBe(false);
      press(row.querySelector('.time')!);
      expect(row.draggable).toBe(true);
    });

    it('rows are drag sources: dragging one selects it and carries the event (for the Notes panel)', async () => {
      const { fixture } = await render();
      const row = at(rows(fixture), 3);
      expect(row.getAttribute('draggable')).toBe('true');
      const data = new Map<string, string>();
      const e = new Event('dragstart', { bubbles: true, cancelable: true }) as DragEvent;
      Object.defineProperty(e, 'dataTransfer', { value: { effectAllowed: 'none', setData: (t: string, v: string) => void data.set(t, v) } });
      row.dispatchEvent(e);
      await settle(fixture);
      const carried = JSON.parse(data.get('application/x-debrief-event') ?? 'null') as { id: number; excerpt: string };
      expect(carried.id).toBeGreaterThanOrEqual(0);
      expect(data.get('text/plain')).toContain(carried.excerpt);
      expect(selectedIndexes(fixture)).toEqual([3]);
      // A drag without a data transfer (some test and automation environments) does nothing.
      const bare = new Event('dragstart', { bubbles: true, cancelable: true });
      expect(() => at(rows(fixture), 4).dispatchEvent(bare)).not.toThrow();
    });

    it('shift+arrow extends the selection from the anchor', async () => {
      const { fixture, store } = await render();
      const vp = el(fixture, '.viewport');
      const key = (k: string, shiftKey = false) => vp.dispatchEvent(new KeyboardEvent('keydown', { key: k, shiftKey, cancelable: true }));
      click(at(rows(fixture), 3));
      key('ArrowDown', true);
      key('ArrowDown', true);
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([3, 4, 5]);
      expect(store.selected()?.id).toBe(5);
      key('ArrowUp', true);
      key('ArrowUp', true);
      key('ArrowUp', true);
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([2, 3]);
      key('ArrowDown');
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([3]);
    });

    it('the context menu acts on all selected rows, or on the clicked row outside the selection', async () => {
      const { fixture, store } = await render();
      const actions = TestBed.inject(RecordActions);
      const open = vi.spyOn(actions, 'openFor').mockImplementation(() => undefined);
      const openAt = vi.spyOn(actions, 'openAtElement').mockImplementation(() => undefined);
      click(at(rows(fixture), 1));
      click(at(rows(fixture), 3), { shiftKey: true });
      await settle(fixture);
      at(rows(fixture), 2).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 5, clientY: 5 }));
      await settle(fixture);
      expect(at(open.mock.calls, 0)[1].id).toBe(2);
      expect(at(open.mock.calls, 0)[2]?.records?.map((r) => r.id)).toEqual([1, 2, 3]);
      expect(selectedIndexes(fixture)).toEqual([1, 2, 3]);
      expect(store.selected()?.id).toBe(2);

      el(fixture, '.viewport').dispatchEvent(new KeyboardEvent('keydown', { key: 'ContextMenu', cancelable: true }));
      expect(at(openAt.mock.calls, 0)[1].id).toBe(2);
      expect(at(openAt.mock.calls, 0)[2]?.records?.map((r) => r.id)).toEqual([1, 2, 3]);
      expect(at(openAt.mock.calls, 0)[0]).toBe(rows(fixture)[2]);

      at(rows(fixture), 7).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 5, clientY: 5 }));
      await settle(fixture);
      expect(at(open.mock.calls, 1)[2]?.records?.map((r) => r.id)).toEqual([7]);
      expect(selectedIndexes(fixture)).toEqual([7]);
    });

    it('collapses to the record selected elsewhere (timeline, bookmark) and clears on new data', async () => {
      const { fixture, store } = await render();
      click(at(rows(fixture), 1));
      click(at(rows(fixture), 4), { shiftKey: true });
      await settle(fixture);
      store.jumpTo(8);
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([8]);
      click(at(rows(fixture), 9), { shiftKey: true });
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([8, 9]);
      store.select(null);
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([]);
      click(at(rows(fixture), 0));
      click(at(rows(fixture), 2), { shiftKey: true });
      await settle(fixture);
      store.select(store.selected() && { ...store.selected()!, id: 10 });
      await settle(fixture);
      expect(selectedIndexes(fixture)).toEqual([10]);
    });

    it('publishes the selected records to GridSelection for commands outside the grid', async () => {
      const { fixture, store } = await render();
      const published = TestBed.inject(GridSelection);
      expect(published.records()).toEqual([]);
      click(at(rows(fixture), 1));
      click(at(rows(fixture), 3), { shiftKey: true });
      click(at(rows(fixture), 6), { ctrlKey: true });
      await settle(fixture);
      expect(published.records().map((r) => r.id)).toEqual([1, 2, 3, 6]);
      expect(published.records().map((r) => r.message)).toEqual([1, 2, 3, 6].map((i) => at(rows(fixture), i).querySelector('.msg')!.textContent!.trim()));
      click(at(rows(fixture), 4));
      await settle(fixture);
      expect(published.records().map((r) => r.id)).toEqual([4]);
      store.select(null);
      await settle(fixture);
      expect(published.records()).toEqual([]);
      click(at(rows(fixture), 2));
      await settle(fixture);
      fixture.destroy();
      expect(published.records()).toEqual([]);
    });
  });

  it('paints rows that a highlight rule matches with a colour bar and a tint', async () => {
    const { fixture } = await render();
    const highlights = TestBed.inject(HighlightRules);
    expect(all(fixture, '.row.hl')).toHaveLength(0);
    highlights.add({ field: '@level', op: 'eq', value: 'Error', color: '#f2545b' });
    await settle(fixture);
    const painted = all<HTMLElement>(fixture, '.row.hl');
    expect(painted).toHaveLength(1);
    expect(at(painted, 0).style.getPropertyValue('--hl')).toBe('#f2545b');
    expect(at(painted, 0).textContent).toContain('ERR');
    at(painted, 0).click();
    await settle(fixture);
    expect(at(painted, 0).classList.contains('sel')).toBe(true);
    highlights.update(at(highlights.items(), 0).id, { enabled: false });
    await settle(fixture);
    expect(all(fixture, '.row.hl')).toHaveLength(0);
    expect(rows(fixture).every((r) => !r.style.getPropertyValue('--hl'))).toBe(true);
  });

  it('publishes the match count of the query in view, never one for an older query (D3)', async () => {
    const { fixture, store } = await render();
    expect(store.matchTotal()).toBe(11);
    store.setTextInput('no event says this');
    store.toggleLevel('Error');
    fixture.detectChanges();
    // The new query's first page is not here yet: no count rather than the old one.
    expect(store.matchTotal()).toBeNull();
    await settle(fixture);
    expect(el(fixture, '.total').textContent).toBe(String(store.matchTotal()));
    store.toggleLevel('Error');
    store.setTextInput('');
    await settle(fixture);
    expect(store.matchTotal()).toBe(11);
    // With the grid closed nothing keeps the count current, so the status bar hides it.
    fixture.destroy();
    expect(store.matchTotal()).toBeNull();
  });

  it('tells the inspector when a filter change hides the selected event (D4)', async () => {
    const { fixture, store } = await render();
    const info = (await TestBed.inject(DebriefBackend).queryPage({ query: store.effectiveQuery(), offset: 0, limit: 50, descending: false })).rows.find((r) => r.level === 'Information')!;
    store.select(info);
    store.toggleLevel('Error');
    await settle(fixture);
    expect(store.selectedHidden()).toBe(true);
    expect(store.selected()?.id).toBe(info.id);
    store.toggleLevel('Error');
    await settle(fixture);
    expect(store.selectedHidden()).toBe(false);
  });

  it('re-queries when the filter or sort changes', async () => {
    const { fixture, store } = await render();
    store.toggleLevel('Error');
    await settle(fixture);
    expect(rows(fixture)).toHaveLength(1);
    store.toggleLevel('Error');
    buttonByText(fixture, /Oldest first/).click();
    await settle(fixture);
    expect(store.descending()).toBe(true);
    expect(at(rows(fixture), 0).textContent).toContain('line without a timestamp');
    el<HTMLButtonElement>(fixture, '.hcell .sort').click();
    await settle(fixture);
    expect(store.descending()).toBe(false);
    store.addFilter({ field: 'MachineName', op: 'eq', value: 'none' });
    await settle(fixture);
    expect(text(fixture)).toContain('No events match');
  });

  it('jumps to anchors and reports hidden ones', async () => {
    const { fixture, store } = await render();
    store.jumpTo(6);
    await settle(fixture);
    expect(store.selected()?.id).toBe(6);
    store.toggleLevel('Fatal');
    await settle(fixture);
    store.jumpTo(0);
    await settle(fixture);
    expect(store.anchorMissed()).toBe(true);
    expect(text(fixture)).toContain('hidden by the current filters');
  });

  it('keeps a jump the query changed under: it asks again against the new query and selects the record', async () => {
    // A recovery reload restores the query, then the selection (a jump); the search box's debounce and smart search then
    // change the query while that jump's answer is on its way (startup-contract.e2e.ts, run 8056).
    const backend = tinyBackend();
    const { fixture, store } = await render(backend);
    const real = backend.queryPage.bind(backend);
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    let holdNext = true;
    vi.spyOn(backend, 'queryPage').mockImplementation(async (req) => {
      if (req.anchor != null && holdNext) {
        holdNext = false;
        await held;
      }
      return real(req);
    });
    store.jumpTo(6);
    await settle(fixture);
    store.patchQuery({ textMode: 'words' });
    await settle(fixture);
    release();
    await settle(fixture);
    expect(store.selected()?.id).toBe(6);
    expect(store.anchorMissed()).toBe(false);
    expect(all(fixture, '.row.sel')).toHaveLength(1);
    // The log shows the jump was asked again (ER-27), with no user content.
    // The scroll writes of the grid log too (ADR 0072): this is about the jump's own lines.
    expect(testLogger().entries.filter((e) => e.category === 'ui.grid' && /jump/i.test(e.message)).map((e) => e.message)).toEqual(['Query changed during a jump; asking again', 'Jump resolved']);
  });

  it('a newer jump wins over an older one still on its way', async () => {
    const backend = tinyBackend();
    const { fixture, store } = await render(backend);
    const real = backend.queryPage.bind(backend);
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    vi.spyOn(backend, 'queryPage').mockImplementation(async (req) => {
      if (req.anchor === 6) await held;
      return real(req);
    });
    store.jumpTo(6);
    await settle(fixture);
    store.jumpTo(3);
    await settle(fixture);
    expect(store.selected()?.id).toBe(3);
    release();
    await settle(fixture);
    expect(store.selected()?.id).toBe(3);
  });

  /** Holds the answers to jump (anchor) requests until `release`; other requests answer at once. */
  function holdJumps(backend: MockBackend, onAnswer?: () => void) {
    const real = backend.queryPage.bind(backend);
    const jumps: PageRequest[] = [];
    let release!: (fail?: unknown) => void;
    let held = new Promise<unknown>((r) => (release = r));
    let holding = true;
    vi.spyOn(backend, 'queryPage').mockImplementation(async (req) => {
      if (req.anchor == null) return real(req);
      jumps.push(req);
      if (holding) {
        holding = false;
        const fail = await held;
        if (fail) throw fail;
      }
      const answer = await real(req);
      onAnswer?.();
      return answer;
    });
    return {
      jumps,
      release: (fail?: unknown) => release(fail),
      holdNext: () => {
        holding = true;
        held = new Promise<unknown>((r) => (release = r));
      },
    };
  }

  it('a jump whose query changes on every answer still ends: at most 3 asks, then an error and a log line', async () => {
    const backend = tinyBackend();
    const { fixture, store } = await render(backend);
    let flip = false;
    const { jumps, release } = holdJumps(backend, () => {
      flip = !flip;
      store.patchQuery({ textMode: flip ? 'words' : undefined });
    });
    store.jumpTo(6);
    release();
    await settle(fixture, 12);
    expect(jumps).toHaveLength(3);
    expect(store.selected()).toBeNull();
    expect(el(fixture, '.err-card').textContent).toContain('the filters kept changing');
    expect(testLogger().entries.some((e) => e.category === 'ui.grid' && e.level === 'warn')).toBe(true);
  });

  it('a jump still on its way when the grid is destroyed neither asks again nor selects', async () => {
    const backend = tinyBackend();
    const { fixture, store } = await render(backend);
    const hold = holdJumps(backend);
    store.jumpTo(6);
    await settle(fixture);
    store.patchQuery({ textMode: 'words' });
    fixture.destroy();
    hold.release();
    await settle(undefined, 6);
    expect(hold.jumps).toHaveLength(1);
    expect(store.selected()).toBeNull();
  });

  it('a jump whose answer failed after the query changed asks again instead of ending silently', async () => {
    const backend = tinyBackend();
    const { fixture, store } = await render(backend);
    const hold = holdJumps(backend);
    store.jumpTo(6);
    await settle(fixture);
    store.patchQuery({ textMode: 'words' });
    await settle(fixture);
    hold.release(new Error('the old query was cancelled'));
    await settle(fixture, 6);
    expect(store.selected()?.id).toBe(6);
    expect(all(fixture, '.err-card')).toHaveLength(0);
  });

  it("the user's click wins over a jump answer that arrives later", async () => {
    const backend = tinyBackend();
    const { fixture, store } = await render(backend);
    const hold = holdJumps(backend);
    store.jumpTo(6);
    await settle(fixture);
    at(rows(fixture), 2).click();
    await settle(fixture);
    expect(store.selected()?.id).toBe(2);
    hold.release();
    await settle(fixture, 6);
    expect(store.selected()?.id).toBe(2);
  });

  it('adds and removes property columns', async () => {
    const { fixture, store } = await render();
    buttonByText(fixture, /Columns/).click();
    await settle(fixture);
    const filter = el<HTMLInputElement>(fixture, '.cols input.input');
    filter.value = 'machine';
    filter.dispatchEvent(new Event('input'));
    await settle(fixture);
    const boxes = all<HTMLInputElement>(fixture, '.col-opt input');
    expect(boxes).toHaveLength(1);
    at(boxes, 0).click();
    await settle(fixture);
    expect(store.extraColumns()).toEqual(['MachineName']);
    expect(at(all(fixture, '.row .extra'), 0).textContent).toBe('WEB01');
    at(all<HTMLInputElement>(fixture, '.col-opt input'), 0).click();
    expect(store.extraColumns()).toEqual([]);
    store.addColumn('@source');
    store.addColumn('@line');
    store.addColumn('@template');
    await settle(fixture);
    const cells = at(all(fixture, '.row'), 0).querySelectorAll('.extra');
    expect([...cells].map((c) => c.textContent)).toEqual(['app.clef', '1', 'Application starting — {Application} {Version} ({Framework})']);
    el<HTMLButtonElement>(fixture, '.hcell.extra .x').click();
    expect(store.extraColumns()).toEqual(['@line', '@template']);
  });

  it('virtualises huge results and compresses the scrollbar', async () => {
    const backend = new HugeBackend(2_000_000);
    const { fixture } = await render(backend);
    const vp = el(fixture, '.viewport');
    const spacer = vp.lastElementChild as HTMLElement;
    expect(parseFloat(spacer.style.height)).toBeLessThanOrEqual(10_000_000);
    expect(rows(fixture).length).toBeLessThan(40);
    vp.scrollTop = parseFloat(spacer.style.height);
    userScroll(vp);
    await scrollIdle(fixture);
    const last = rows(fixture).at(-1)!;
    expect(last.textContent).toContain('row 1999999');
    expect(backend.requests.some((r) => r.offset === 3999 * 500)).toBe(true);
  });

  it('loads pages on demand while selecting with the keyboard', async () => {
    const backend = new HugeBackend(5000);
    const { fixture, store } = await render(backend);
    const vp = el(fixture, '.viewport');
    vp.dispatchEvent(new KeyboardEvent('keydown', { key: 'End' }));
    await settle(fixture);
    await new Promise((r) => setTimeout(r, 120));
    expect(store.selected()?.id).toBe(4999);
    store.jumpTo(2500);
    await settle(fixture);
    expect(store.selected()?.id).toBe(2500);
  });

  it('reports page errors', async () => {
    const backend = tinyBackend();
    const { fixture, store } = await render(backend);
    const query = vi.spyOn(backend, 'queryPage').mockRejectedValueOnce('page failed');
    store.toggleLevel('Error');
    await settle(fixture);
    expect(el(fixture, '.err-card[role=alert]').textContent).toContain('page failed');
    expect(store.error()).toBeNull();
    buttonByText(fixture, 'Retry').click();
    await settle(fixture);
    expect(query).toHaveBeenCalledTimes(2);
    expect(all(fixture, '.err-card')).toHaveLength(0);
    expect(rows(fixture)).toHaveLength(1);
  });
});

describe('EventsGrid at scale', { timeout: 30_000 }, () => {
  const ROW = 24;
  const BIG = 50_000_000;
  // Restored after each test (restoreMocks in vitest-base.config.mts).
  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
        return this.classList?.contains('viewport') ? VIEWPORT_H + 28 : 0;
    });
  });
  beforeEach(() => localStorage.clear());

  function synthetic(total = BIG, extra: ConstructorParameters<typeof MockBackend>[0] = {}) {
    return new MockBackend({ latency: false, dataset: tinyDataset, syntheticTotal: total, ...extra });
  }

  async function render(backend: DebriefBackend) {
    TestBed.configureTestingModule({ providers: [provideTinyBackend(backend)] });
    const fixture = TestBed.createComponent(EventsGrid);
    const store = TestBed.inject(WorkspaceStore);
    await settle(fixture);
    const grid = fixture.componentInstance as unknown as { virtualTop(): number; rows: { pageCount: number }; total(): number; geometry(): { maxTop: number; maxScroll: number; scaled: boolean } };
    const vp = el(fixture, '.viewport');
    return { fixture, store, grid, vp };
  }

  const visibleIds = (f: Awaited<ReturnType<typeof render>>['fixture']) =>
    all(f, '.row:not(.placeholder)').map((r) => Number(/Synthetic event (\d+)/.exec(r.textContent ?? '')?.[1]));
  const key = (vp: HTMLElement, k: string) => vp.dispatchEvent(new KeyboardEvent('keydown', { key: k, cancelable: true }));
  const flush = async (f: Awaited<ReturnType<typeof render>>['fixture']) => {
    await settle(f);
    await scrollIdle(f);
  };

  it('shows the full total and a capped spacer for 50 million rows', async () => {
    const { fixture, grid, vp } = await render(synthetic());
    expect(el(fixture, '.total').textContent).toBe('50,000,000');
    expect(grid.geometry().scaled).toBe(true);
    expect(parseFloat((vp.lastElementChild as HTMLElement).style.height)).toBe(10_000_000 - VIEWPORT_H);
    expect(visibleIds(fixture)[0]).toBe(0);
  });

  it('jumps to the end with the scrollbar and shows the very last rows', async () => {
    const { fixture, grid, vp } = await render(synthetic());
    vp.scrollTop = grid.geometry().maxScroll;
    userScroll(vp);
    await flush(fixture);
    expect(grid.virtualTop()).toBe(grid.geometry().maxTop);
    const ids = visibleIds(fixture);
    expect(ids.at(-1)).toBe(BIG - 1);
  });

  it('moves the scrollbar thumb proportionally', async () => {
    const { fixture, grid, vp } = await render(synthetic());
    vp.scrollTop = grid.geometry().maxScroll / 2;
    userScroll(vp);
    await flush(fixture);
    expect(Math.abs(grid.virtualTop() / ROW - BIG / 2)).toBeLessThan(VIEWPORT_H / ROW + 1);
    expect(visibleIds(fixture).some((i) => Math.abs(i - BIG / 2) < 40)).toBe(true);
  });

  it('lands exactly on an anchor near 40 million and selects it', async () => {
    const { fixture, store, grid } = await render(synthetic());
    const id = 40_000_123;
    store.jumpTo(id);
    await flush(fixture);
    expect(store.selected()?.id).toBe(id);
    expect(visibleIds(fixture)).toContain(id);
    const row = at(all(fixture, '.row.sel'), 0);
    expect(row).toBeDefined();
    const top = parseFloat(row.style.top);
    expect(top).toBeGreaterThan(0);
    expect(top + ROW).toBeLessThanOrEqual(VIEWPORT_H);
    // Centred on the row, within half a row.
    expect(Math.abs(top - (VIEWPORT_H - ROW) / 2)).toBeLessThan(ROW);
    expect(grid.virtualTop()).toBeCloseTo(id * ROW - VIEWPORT_H / 2 + ROW / 2, 3);
  });

  it('lands on an anchor in descending order too', async () => {
    const { fixture, store } = await render(synthetic());
    store.toggleSort();
    await flush(fixture);
    expect(visibleIds(fixture)[0]).toBe(BIG - 1);
    store.jumpTo(10_000_000);
    await flush(fixture);
    expect(store.selected()?.id).toBe(10_000_000);
    expect(visibleIds(fixture)).toContain(10_000_000);
  });

  it('navigates by keyboard: arrows, pages, Home and End', async () => {
    const { fixture, store, grid, vp } = await render(synthetic());
    key(vp, 'End');
    await flush(fixture);
    expect(store.selected()?.id).toBe(BIG - 1);
    expect(grid.virtualTop()).toBe(grid.geometry().maxTop);
    expect(visibleIds(fixture).at(-1)).toBe(BIG - 1);
    key(vp, 'ArrowUp');
    expect(store.selected()?.id).toBe(BIG - 2);
    key(vp, 'PageUp');
    await flush(fixture);
    expect(store.selected()?.id).toBe(BIG - 2 - 9);
    key(vp, 'Home');
    await flush(fixture);
    expect(store.selected()?.id).toBe(0);
    expect(grid.virtualTop()).toBe(0);
    key(vp, 'ArrowDown');
    key(vp, 'PageDown');
    await flush(fixture);
    expect(store.selected()?.id).toBe(1 + 9);
    for (let i = 0; i < 40; i++) key(vp, 'ArrowDown');
    await flush(fixture);
    expect(store.selected()?.id).toBe(50);
    // The selection stays on screen as the rows scroll under it.
    const sel = at(all(fixture, '.row.sel'), 0);
    expect(parseFloat(sel.style.top) + ROW).toBeLessThanOrEqual(VIEWPORT_H + 0.5);
  });

  it('steps the wheel by pixels of the true content, not of the compressed spacer', async () => {
    const { fixture, grid, vp } = await render(synthetic());
    const wheel = (deltaY: number, deltaMode = 0, ctrlKey = false) => {
      const e = new WheelEvent('wheel', { deltaY, deltaMode, ctrlKey, cancelable: true });
      vp.dispatchEvent(e);
      return e;
    };
    const e = wheel(52);
    expect(e.defaultPrevented).toBe(true);
    expect(grid.virtualTop()).toBe(52);
    wheel(104);
    expect(grid.virtualTop()).toBe(156);
    wheel(1, 1);
    expect(grid.virtualTop()).toBe(156 + 3 * ROW);
    wheel(-100000);
    expect(grid.virtualTop()).toBe(0);
    expect(wheel(0).defaultPrevented).toBe(false);
    expect(wheel(10, 0, true).defaultPrevented).toBe(false);
    await flush(fixture);
    expect(visibleIds(fixture)[0]).toBe(0);
  });

  it('leaves the wheel to the browser while the content fits', async () => {
    const { vp } = await render(synthetic(1000));
    const e = new WheelEvent('wheel', { deltaY: 40, cancelable: true });
    vp.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
  });

  it('does not let rounding of the scrollbar position move a row-exact position', async () => {
    const { fixture, grid, vp } = await render(synthetic());
    const e = new WheelEvent('wheel', { deltaY: 30, cancelable: true });
    vp.dispatchEvent(e);
    const top = grid.virtualTop();
    userScroll(vp);
    expect(grid.virtualTop()).toBe(top);
    // A scroll the user caused (somewhere else) is followed.
    vp.scrollTop = 5_000_000;
    userScroll(vp);
    await flush(fixture);
    expect(grid.virtualTop()).toBeGreaterThan(1e8);
  });

  it('keeps memory bounded while scrolling through millions of rows', async () => {
    const { fixture, grid, vp } = await render(synthetic());
    for (let i = 1; i <= 50; i++) {
      vp.scrollTop = i * 150_000;
      userScroll(vp);
      await settle(fixture, 2);
    }
    await flush(fixture);
    expect(grid.rows.pageCount).toBeLessThanOrEqual(40);
    expect(visibleIds(fixture).length).toBeGreaterThan(10);
  });

  it('shows an inline error with Retry when jumping fails, and when the first page cannot load', async () => {
    const backend = synthetic();
    const { fixture, store } = await render(backend);
    vi.spyOn(backend, 'queryPage').mockRejectedValueOnce('jump failed');
    store.jumpTo(5);
    await flush(fixture);
    expect(el(fixture, '.err-card').textContent).toContain('jump failed');
    buttonByText(fixture, 'Retry').click();
    await flush(fixture);
    expect(all(fixture, '.err-card')).toHaveLength(0);
  });

  it('retries the first page when nothing has loaded yet', async () => {
    const backend = withMockFailures(synthetic(BIG), { latency: false, fail: { queryPage: 1 } });
    const { fixture } = await render(backend);
    expect(el(fixture, '.err-card[role=alert]').textContent).toContain('Simulated queryPage failure');
    expect(el(fixture, '.total').textContent).toBe('0');
    buttonByText(fixture, 'Retry').click();
    await flush(fixture);
    expect(all(fixture, '.err-card')).toHaveLength(0);
    expect(el(fixture, '.total').textContent).toBe('50,000,000');
  });

  it('fetches pages once scrolling pauses, not for every position passed', async () => {
    const backend = synthetic();
    const { fixture, vp } = await render(backend);
    const query = vi.spyOn(backend, 'queryPage');
    // A fake clock: on a loaded machine the wall time between two scroll events can pass the pause by itself.
    vi.useFakeTimers();
    try {
      for (let i = 1; i <= 20; i++) {
        vp.scrollTop = i * 150_000;
        userScroll(vp);
        fixture.detectChanges();
        await vi.advanceTimersByTimeAsync(10);
      }
      expect(query).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(2_000);
    } finally {
      vi.useRealTimers();
    }
    await flush(fixture);
    // Before: every position passed asked for the pages it showed.
    expect(query.mock.calls.length).toBeGreaterThan(0);
    expect(query.mock.calls.length).toBeLessThanOrEqual(2);
    expect(visibleIds(fixture).length).toBeGreaterThan(10);
  });

  it('does not retry a failed page on scroll, only on Retry', async () => {
    const backend = synthetic();
    const { fixture, vp } = await render(backend);
    const query = vi.spyOn(backend, 'queryPage').mockRejectedValue('engine down');
    vp.scrollTop = 4_000_000;
    userScroll(vp);
    await flush(fixture);
    expect(el(fixture, '.err-card').textContent).toContain('engine down');
    const failedCalls = query.mock.calls.length;
    for (const top of [4_000_010, 4_000_020, 4_000_030]) {
      vp.scrollTop = top;
      userScroll(vp);
      await flush(fixture);
    }
    expect(query.mock.calls.length).toBe(failedCalls);
    query.mockRestore();
    buttonByText(fixture, 'Retry').click();
    await flush(fixture);
    expect(all(fixture, '.err-card')).toHaveLength(0);
    expect(visibleIds(fixture).length).toBeGreaterThan(10);
  });

  it('retries a page that failed while scrolling', async () => {
    const backend = synthetic();
    const { fixture, grid, vp } = await render(backend);
    vi.spyOn(backend, 'queryPage').mockRejectedValueOnce('far page failed');
    vp.scrollTop = 4_000_000;
    userScroll(vp);
    await flush(fixture);
    expect(el(fixture, '.err-card').textContent).toContain('far page failed');
    buttonByText(fixture, 'Retry').click();
    await flush(fixture);
    expect(all(fixture, '.err-card')).toHaveLength(0);
    expect(visibleIds(fixture).length).toBeGreaterThan(10);
    expect(grid.virtualTop()).toBeGreaterThan(0);
  });

  describe('live tail', () => {
    /** Moves the feed's clock one tick at a time until `done()`, letting the grid take each batch (a bounded number of ticks). */
    async function tickUntil(clock: ManualClock, fixture: Awaited<ReturnType<typeof render>>['fixture'], done: () => boolean) {
      for (let i = 0; i < 40 && !done(); i++) {
        await clock.advance(25);
        await flush(fixture);
      }
      expect(done()).toBe(true);
    }

    async function live(descending: boolean, scrollRows: number | 'end') {
      const clock = new ManualClock();
      const backend = synthetic(5000, { liveMs: 25, clock });
      const { fixture, store, grid, vp } = await render(backend);
      if (descending) store.toggleSort();
      await flush(fixture);
      if (scrollRows === 'end') key(vp, 'End');
      else if (scrollRows > 0) {
        vp.scrollTop = scrollRows * ROW;
        userScroll(vp);
      }
      await flush(fixture);
      const before = { total: grid.total(), top: grid.virtualTop(), ids: visibleIds(fixture) };
      const tail = TestBed.inject(LiveTail);
      await tail.toggle();
      expect(tail.enabled()).toBe(true);
      await tickUntil(clock, fixture, () => grid.total() > before.total + 1);
      await tail.toggle();
      await flush(fixture);
      return { fixture, store, grid, before, tail };
    }

    it('keeps the position when scrolled away in ascending order', async () => {
      const { grid, before, fixture } = await live(false, 1000);
      expect(grid.virtualTop()).toBe(before.top);
      expect(visibleIds(fixture)[0]).toBe(before.ids[0]);
    });

    it('keeps the same events in view in descending order when scrolled away (new events are inserted above)', async () => {
      const { grid, before, fixture } = await live(true, 1000);
      expect(grid.total()).toBeGreaterThan(before.total);
      expect(grid.virtualTop()).toBeGreaterThan(before.top);
      expect(visibleIds(fixture)).toContain(before.ids[Math.floor(before.ids.length / 2)]);
    });

    it('follows new events at the top in descending order', async () => {
      const { grid, fixture, before } = await live(true, 0);
      expect(grid.virtualTop()).toBe(0);
      expect(visibleIds(fixture)[0]).toBeGreaterThan(at(before.ids, 0));
    });

    it('follows new events at the end in ascending order', async () => {
      const { grid, fixture } = await live(false, 'end');
      expect(grid.virtualTop()).toBe(grid.geometry().maxTop);
      expect(visibleIds(fixture).at(-1)).toBe(grid.total() - 1);
    });

    it('has no live control of its own (the status bar owns it)', async () => {
      const { fixture } = await render(synthetic(5000));
      expect(all(fixture, 'button').some((b) => /live/i.test(b.textContent ?? ''))).toBe(false);
    });

    it('holds the rows while not following, counts the new events, and reveals the newest on request', async () => {
      for (const descending of [true, false]) {
        TestBed.resetTestingModule();
        // The backend's pushes run on a clock the test moves (ER-28; measured in 8a5ad864a on fix/debrief-events-grid-live-tail):
        // the reveal scrolls to the end of the rows there are when it runs, and with a real timer a push after it moved that end
        // by one row (24 px) before the assertion read it (120052 against 120028, once in about three full runs; run 8191 too).
        const clock = new ManualClock();
        const backend = synthetic(5000, { liveMs: 25, clock });
        const { fixture, store, grid } = await render(backend);
        if (descending) store.toggleSort();
        await flush(fixture);
        TestBed.inject(LiveFollow).setFollow(false);
        const feed = TestBed.inject(LiveFeed);
        const tail = TestBed.inject(LiveTail);
        await tail.toggle();
        await clock.advance(100);
        await vi.waitFor(() => expect(feed.newEvents()).toBeGreaterThan(1), { timeout: 3000 });
        expect(grid.total()).toBe(5000);
        await settle(fixture);
        el<HTMLButtonElement>(fixture, '[data-testid=new-events]').click();
        await vi.waitFor(() => expect(grid.total()).toBeGreaterThan(5000), { timeout: 3000 });
        await flush(fixture);
        expect(grid.virtualTop()).toBe(descending ? 0 : grid.geometry().maxTop);
        await tail.toggle();
      }
    });

    // Product bug (lessons.md 2026-11-16): the reveal asks for several pages in one round; an event pushed between two of the
    // answers made a later page report a larger total than the one the scroll to the end was made for, and the grid stayed one
    // row (24 px) short of the newest event. The order is forced here: the second page of the reveal's round is answered after a push.
    it('reveals the newest event even when a push lands between the pages of the reveal\'s refresh', async () => {
      TestBed.resetTestingModule();
      const clock = new ManualClock();
      const backend = synthetic(5000, { liveMs: 25, clock });
      const { fixture, grid } = await render(backend);
      await flush(fixture);
      TestBed.inject(LiveFollow).setFollow(false);
      const feed = TestBed.inject(LiveFeed);
      await TestBed.inject(LiveTail).toggle();
      await clock.advance(100);
      await vi.waitFor(() => expect(feed.newEvents()).toBeGreaterThan(1));
      await settle(fixture);
      const answer = backend.queryPage.bind(backend);
      let asked = 0;
      vi.spyOn(backend, 'queryPage').mockImplementation(async (request) => {
        // The reveal's first page is answered as asked; a push lands before the second page is read.
        if (asked++ === 1) await clock.advance(25);
        return answer(request);
      });
      el<HTMLButtonElement>(fixture, '[data-testid=new-events]').click();
      await vi.waitFor(() => expect(asked).toBeGreaterThan(1));
      await flush(fixture);
      const total = grid.total();
      expect(total).toBeGreaterThan(5000);
      expect(grid.virtualTop(), `the view ends at the last of ${total} rows`).toBe(grid.geometry().maxTop);
    });

    // The follow-up of a reveal belongs to the reveal's own pages, not to its round: a page fetched because the user scrolled
    // away later (the grid holds new events, so no new round starts) must not jump the view back to the end.
    it('does not pull the view back to the end after the user scrolls away from a reveal', async () => {
      TestBed.resetTestingModule();
      const clock = new ManualClock();
      const backend = synthetic(5000, { liveMs: 25, clock });
      const { fixture, grid, vp } = await render(backend);
      await flush(fixture);
      TestBed.inject(LiveFollow).setFollow(false);
      const feed = TestBed.inject(LiveFeed);
      await TestBed.inject(LiveTail).toggle();
      await clock.advance(100);
      await vi.waitFor(() => expect(feed.newEvents()).toBeGreaterThan(1));
      await settle(fixture);
      el<HTMLButtonElement>(fixture, '[data-testid=new-events]').click();
      await flush(fixture);
      expect(grid.virtualTop()).toBe(grid.geometry().maxTop);
      // The user scrolls to the top; more events arrive and are held; then the user scrolls to the middle.
      TestBed.inject(LiveFollow).setFollow(false);
      await clock.advance(100);
      await vi.waitFor(() => expect(feed.newEvents()).toBeGreaterThan(0));
      vp.scrollTop = 0;
      userScroll(vp);
      await flush(fixture);
      await clock.advance(100);
      vp.scrollTop = 60_000;
      userScroll(vp);
      await flush(fixture);
      await settle(fixture);
      expect(grid.virtualTop()).toBeLessThan(70_000);
    });

    // ADR 0072. The reveal with its second page held back (answered after a push); `release()` answers it.
    async function revealWithHeldPage() {
      TestBed.resetTestingModule();
      const clock = new ManualClock();
      const backend = synthetic(5000, { liveMs: 25, clock });
      const { fixture, grid, vp } = await render(backend);
      await flush(fixture);
      TestBed.inject(LiveFollow).setFollow(false);
      const feed = TestBed.inject(LiveFeed);
      await TestBed.inject(LiveTail).toggle();
      await clock.advance(100);
      await vi.waitFor(() => expect(feed.newEvents()).toBeGreaterThan(1));
      await settle(fixture);
      const answer = backend.queryPage.bind(backend);
      let asked = 0;
      let release: () => void = () => undefined;
      const held = new Promise<void>((resolve) => (release = resolve));
      vi.spyOn(backend, 'queryPage').mockImplementation(async (request) => {
        // The first page of the reveal is answered as asked; the second waits until the test lets it go (after a push).
        if (asked++ === 1) await held;
        return answer(request);
      });
      el<HTMLButtonElement>(fixture, '[data-testid=new-events]').click();
      await vi.waitFor(() => expect(asked).toBeGreaterThan(1));
      await settle(fixture);
      return { fixture, grid, vp, clock, release: async () => { await clock.advance(25); release(); await flush(fixture); } };
    }

    it('G1: a key move during a reveal ends its hold: a later page with a larger total does not pull the view back', async () => {
      const { fixture, grid, vp, release } = await revealWithHeldPage();
      key(vp, 'PageUp');
      await settle(fixture);
      const left = grid.virtualTop();
      expect(left).toBeLessThan(grid.geometry().maxTop);
      await release();
      expect(grid.virtualTop(), 'the view stays where the user moved it').toBeLessThan(grid.geometry().maxTop - 5 * ROW);
    });

    it('G1: a wheel in scaled mode and a click on the scrollbar are user take-overs too', async () => {
      const { fixture, grid, vp, release } = await revealWithHeldPage();
      vp.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
      vp.dispatchEvent(new WheelEvent('wheel', { deltaY: -480, cancelable: true, bubbles: true }));
      vp.scrollTop = Math.max(0, vp.scrollTop - 480);
      userScroll(vp);
      await settle(fixture);
      await release();
      expect(grid.virtualTop()).toBeLessThan(grid.geometry().maxTop - 5 * ROW);
    });

    it('G9: a page fetched after the reveal that reports a larger total keeps the pinned view on the newest row', async () => {
      const { fixture, grid, clock, release } = await revealWithHeldPage();
      await release();
      expect(grid.virtualTop()).toBe(grid.geometry().maxTop);
      // An event is pushed; a page the view did not ask for before is fetched (the engine reads the new total with it).
      await clock.advance(25);
      (grid as unknown as { rows: { ensurePage(page: number): void } }).rows.ensurePage(2);
      await flush(fixture);
      expect(grid.virtualTop()).toBe(grid.geometry().maxTop);
    });

    // Found by the e2e (WebKit, wheel dispatched right after the click): the click's request is handled when the change detection runs,
    // which can be after the user's wheel. The wheel came later and wins: the request still reloads the rows but does not pin the view.
    it('G10: a wheel that arrives before the reveal request is handled is not undone by it', async () => {
      TestBed.resetTestingModule();
      const clock = new ManualClock();
      const backend = synthetic(5000, { liveMs: 25, clock });
      const { fixture, grid, vp } = await render(backend);
      await flush(fixture);
      TestBed.inject(LiveFollow).setFollow(false);
      const feed = TestBed.inject(LiveFeed);
      await TestBed.inject(LiveTail).toggle();
      await clock.advance(100);
      await vi.waitFor(() => expect(feed.newEvents()).toBeGreaterThan(1));
      await settle(fixture);
      const before = grid.virtualTop();
      el<HTMLButtonElement>(fixture, '[data-testid=new-events]').click();
      vp.dispatchEvent(new WheelEvent('wheel', { deltaY: -900, cancelable: true, bubbles: true }));
      vp.scrollTop = Math.max(0, vp.scrollTop - 900);
      userScroll(vp);
      await flush(fixture);
      await settle(fixture);
      expect(grid.virtualTop(), 'the view is not taken to the end').toBeLessThan(grid.geometry().maxTop - 5 * ROW);
      expect(before).toBeLessThan(grid.geometry().maxTop - 5 * ROW);
    });

    // G10b: Follow on and Resume are requests too: the stamp is taken when the user acts (in the store), not when the feed's effect runs.
    for (const how of ['Follow on', 'Resume'] as const) {
      it(`G10b: a wheel after ${how} is pressed but before its effect runs is not undone`, async () => {
        TestBed.resetTestingModule();
        const clock = new ManualClock();
        const backend = synthetic(5000, { liveMs: 25, clock });
        const { fixture, grid, vp } = await render(backend);
        await flush(fixture);
        const follow = TestBed.inject(LiveFollow);
        if (how === 'Follow on') follow.setFollow(false);
        else follow.pause();
        const feed = TestBed.inject(LiveFeed);
        await TestBed.inject(LiveTail).toggle();
        await clock.advance(100);
        await vi.waitFor(() => expect(feed.newEvents()).toBeGreaterThan(1));
        await settle(fixture);
        if (how === 'Follow on') follow.setFollow(true);
        else follow.resume();
        vp.dispatchEvent(new WheelEvent('wheel', { deltaY: -900, cancelable: true, bubbles: true }));
        vp.scrollTop = Math.max(0, vp.scrollTop - 900);
        userScroll(vp);
        await flush(fixture);
        await settle(fixture);
        expect(grid.virtualTop(), 'the view is not taken to the end').toBeLessThan(grid.geometry().maxTop - 5 * ROW);
      });
    }

    // The classifier's wiring (scroll-classify.spec.ts holds its table): `onScroll` says whether the layout is changing. The browser's scroll metrics
    // are stubbed (jsdom has none): the scrollable range is the spacer's height, and the grid's last write saw `grown` px less of it.
    describe('a scroll with no input event', () => {
      type Internals = { inputSinceWrite: boolean; layoutDirty: boolean; writtenScroll: number; writtenMax: number; writtenClient: number; pendingWrite: unknown };
      async function viewAfterGrowth(grown: number) {
        TestBed.resetTestingModule();
        vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockImplementation(function (this: HTMLElement) {
          return this.classList?.contains('viewport') ? parseFloat((this.lastElementChild as HTMLElement | null)?.style.height || '0') + VIEWPORT_H + 28 : 0;
        });
        const { fixture, grid, vp } = await render(synthetic(5000, { liveMs: 25 }));
        await flush(fixture);
        const internals = grid as unknown as Internals;
        const start = 60 * ROW;
        vp.scrollTop = start;
        userScroll(vp);
        await flush(fixture);
        // The grid's last write saw `grown` px less to scroll: the total grew since, with no write (oldest first, keep mode).
        internals.inputSinceWrite = false;
        internals.writtenScroll = start;
        internals.writtenMax = vp.scrollHeight - vp.clientHeight - grown;
        internals.writtenClient = vp.clientHeight;
        return { fixture, grid, vp, internals, start };
      }

      it('is kept once the frame that renders a layout change has passed (find in page, assistive technology)', async () => {
        const { grid, vp, internals, start } = await viewAfterGrowth(600);
        await vi.waitFor(() => expect(internals.layoutDirty, 'the frame after the render has passed').toBe(false));
        vp.scrollTop = start + 300; // inside the growth band, no wheel, key or pointer before it
        vp.dispatchEvent(new Event('scroll'));
        expect(grid.virtualTop(), 'the user\'s position, not written back').toBe(start + 300);
        expect(internals.pendingWrite).toBeNull();
      });

      it('is the browser\'s own adjustment while the layout is changing: the grid\'s position is written back', async () => {
        const { grid, vp, internals, start } = await viewAfterGrowth(600);
        internals.layoutDirty = true;
        vp.scrollTop = start + 300;
        vp.dispatchEvent(new Event('scroll'));
        expect(grid.virtualTop(), 'the grid\'s own position stays').toBe(start);
        expect(internals.pendingWrite).not.toBeNull();
      });
    });

    it('G2: a deferred write is dropped when the user moves the view in the same frame', async () => {
      const { fixture, grid, vp, release } = await revealWithHeldPage();
      // The held page answers (a larger total: a deferred write to the end is scheduled) and, before the render, the user presses PageUp.
      const settled = release();
      key(vp, 'PageUp');
      await settled;
      await settle(fixture);
      expect(vp.scrollTop, 'the scroll position is the user\'s, not the end').toBeLessThan(grid.geometry().maxTop - 5 * ROW);
    });

    // ADR 0072, G4: `pinned` is stored; a resize keeps a pinned view on the newest row and a following grid keeps following.
    it('G4: a smaller viewport while pinned keeps the view on the newest row, and the next push still follows', async () => {
      // `vi.stubGlobal` is not undone by `restoreMocks`: put the real ResizeObserver back when the test ends, pass or fail.
      onTestFinished(() => void vi.unstubAllGlobals());
      TestBed.resetTestingModule();
      let height = VIEWPORT_H;
      vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
        return this.classList?.contains('viewport') ? height : 0;
      });
      let measure: () => void = () => undefined;
      vi.stubGlobal('ResizeObserver', class {
        constructor(cb: () => void) {
          measure = cb;
        }
        observe(): void {
          measure();
        }
        unobserve(): void {}
        disconnect(): void {}
      });
      const clock = new ManualClock();
      const { fixture, grid, vp } = await render(synthetic(5000, { liveMs: 25, clock }));
      await flush(fixture);
      key(vp, 'End'); // to the newest end: the view is pinned
      await flush(fixture);
      await TestBed.inject(LiveTail).toggle();
      await clock.advance(100);
      await flush(fixture);
      expect(grid.virtualTop()).toBe(grid.geometry().maxTop);
      height = VIEWPORT_H - 120;
      measure();
      await flush(fixture);
      expect(grid.virtualTop(), 'a smaller viewport moves the end: the pinned view stays on the newest row').toBe(grid.geometry().maxTop);
      await clock.advance(100);
      await flush(fixture);
      expect(grid.virtualTop(), 'the next push still follows').toBe(grid.geometry().maxTop);
    });

    it('keeps the same rows selected when new events are inserted above them', async () => {
      const clock = new ManualClock();
      const backend = synthetic(5000, { liveMs: 25, clock });
      const { fixture, store, grid, vp } = await render(backend);
      store.toggleSort();
      await flush(fixture);
      vp.scrollTop = 1000 * ROW;
      userScroll(vp);
      await flush(fixture);
      const visible = all<HTMLElement>(fixture, '.row:not(.placeholder)');
      at(visible, 3).click();
      at(visible, 5).dispatchEvent(new MouseEvent('click', { bubbles: true, shiftKey: true }));
      await flush(fixture);
      const selectedIds = () =>
        all(fixture, '.row[aria-selected=true]').map((r) => Number(/Synthetic event (\d+)/.exec(r.textContent ?? '')?.[1]));
      const before = selectedIds();
      expect(before).toHaveLength(3);
      const total = grid.total();
      const tail = TestBed.inject(LiveTail);
      await tail.toggle();
      await tickUntil(clock, fixture, () => grid.total() > total + 1);
      await tail.toggle();
      await flush(fixture);
      expect(selectedIds()).toEqual(before);
    });
  });
});

describe('splitInstance', () => {
  it('splits the name from the host and process id', () => {
    expect(splitInstance('OrderService · WEB01 · 3312')).toEqual({ instName: 'OrderService', instRest: 'WEB01 · 3312' });
    expect(splitInstance('worker')).toEqual({ instName: 'worker', instRest: '' });
  });
});
