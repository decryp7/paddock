import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { provideIcons } from '../../shared/icons';
import { LiveStore } from '../../state/live.store';
import { Lookups } from '../lookup/lookups';
import { ProcessesStore } from '../../state/processes.store';
import { nextFrame } from '../../testing/frame';
import { ProcessesPage } from './processes-page';

const NOW = 1_700_000_000_000;

async function make(os: 'windows' | 'macos' = 'windows', fail?: Set<string>) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, os, fail });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(ProcessesPage);
  const store = TestBed.inject(ProcessesStore);
  TestBed.inject(LiveStore);
  f.detectChanges();
  await f.whenStable();
  await vi.waitFor(() => expect(store.processes().length).toBeGreaterThan(0)); // the mock's first live update
  f.detectChanges();
  await f.whenStable();
  await nextFrame();
  f.detectChanges();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const c = f.componentInstance as any;
  const dialogs = TestBed.inject(DialogService);
  const processRow = () => {
    store.setMode('flat');
    return store.rows().find((r) => r.kind === 'process')!;
  };
  return { f, store, mock, c, dialogs, el: f.nativeElement as HTMLElement, processRow };
}

describe('ProcessesPage actions', () => {
  it('summarises the machine', async () => {
    const { c } = await make();
    expect(c.summary()).toMatch(/processes · .*% CPU · .* memory in use/);
    expect(c.pdSort()).toEqual({ key: 'cpu', direction: 'desc' });
    c.store.sortBy('cpu');
    expect(c.pdSort()).toEqual({ key: 'cpu', direction: 'asc' });
    c.onSort({ key: 'user', direction: 'asc' });
    expect(c.store.sort()).toEqual({ key: 'user', dir: 'asc' });
    c.onSort(null);
    expect(c.store.sort()).toEqual({ key: 'user', dir: 'asc' });
  });

  it('lockers opens the lockers dialog; details selects a process; groups are ignored', async () => {
    const { c, dialogs, store, processRow } = await make();
    const open = vi.spyOn(dialogs, 'open').mockReturnValue({ closed: Promise.resolve({ kind: 'dismissed' }), dismiss() {} } as never);
    c.lockers();
    expect(open).toHaveBeenCalledWith('dy.lockers');
    const select = vi.spyOn(store, 'select');
    const row = processRow();
    c.details(row);
    expect(select).toHaveBeenCalledWith(row.ref);
    c.details({ ref: null });
    expect(select).toHaveBeenCalledTimes(1);
    expect(c.isSelected(row)).toBe(true);
    expect(c.isSelected({ ref: null })).toBe(false);
  });

  it('rowClick selects a process and toggles a group', async () => {
    const { c, store, processRow } = await make();
    const toggle = vi.spyOn(store, 'toggle');
    const select = vi.spyOn(store, 'select');
    c.rowClick(processRow());
    expect(select).toHaveBeenCalled();
    store.setMode('groups');
    const group = store.rows().find((r) => r.kind === 'group' && r.expandable)!;
    c.rowClick(group);
    expect(toggle).toHaveBeenCalledWith(group);
    // A group is folded, not selected: the highlight stays with the open process.
    expect(c.selectedKey()).not.toBe(group.key);
  });

  it('opens the details panel when a row is selected', async () => {
    const { f, el, store, processRow } = await make();
    await store.select(processRow().ref);
    f.detectChanges();
    await f.whenStable();
    f.detectChanges();
    expect(el.querySelector('dy-process-details')).not.toBeNull();
    expect(el.querySelector('.body.open')).not.toBeNull();
  });

  it('applies ?q= to the filter', async () => {
    const { f, store } = await make();
    f.componentRef.setInput('q', 'rustc');
    f.detectChanges();
    expect(store.query()).toBe('rustc');
  });

  describe('keyboard', () => {
    const press = (target: Element, key: string) => target.dispatchEvent(new KeyboardEvent('keydown', { key, cancelable: true, bubbles: true }));
    const rowEls = (el: HTMLElement) => [...el.querySelectorAll<HTMLElement>('[role="row"][data-key]')];
    const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
      await f.whenStable();
      await nextFrame();
      f.detectChanges();
    };

    it('has one tab stop; the arrows move between rows', async () => {
      const { f, el } = await make();
      const rows = rowEls(el);
      expect(rows.length).toBeGreaterThan(2);
      expect(rows.filter((r) => r.tabIndex === 0)).toHaveLength(1);
      rows[0]!.focus();
      press(rows[0]!, 'ArrowDown');
      await settle(f);
      expect(document.activeElement).toBe(rowEls(el)[1]);
      press(rowEls(el)[1]!, 'ArrowUp');
      await settle(f);
      expect(document.activeElement).toBe(rowEls(el)[0]);
    });
    it('says the levels of the tree and Right unfolds a folded app, Left folds an unfolded one', async () => {
      const { f, el, store } = await make();
      store.setMode('groups');
      f.detectChanges();
      await settle(f);
      const grid = el.querySelector('[role="treegrid"]')!;
      expect(grid).not.toBeNull();
      const group = () => rowEls(el).find((r) => r.hasAttribute('aria-expanded'))!;
      expect(group().getAttribute('aria-level')).toBe('1');
      const toggle = vi.spyOn(store, 'toggle');
      group().focus();
      const open = group().getAttribute('aria-expanded') === 'true';
      press(group(), open ? 'ArrowLeft' : 'ArrowRight');
      await settle(f);
      expect(toggle).toHaveBeenCalledTimes(1);
      expect(group().getAttribute('aria-expanded')).toBe(open ? 'false' : 'true');
      // The focus stays on the row that was folded.
      expect(document.activeElement).toBe(group());
    });
    it('Right on an unfolded app moves to its first process and Left comes back', async () => {
      const { f, el, store } = await make();
      store.setMode('groups');
      const g = store.rows().find((r) => r.expandable)!;
      if (!g.expanded) store.toggle(g);
      f.detectChanges();
      await settle(f);
      const group = rowEls(el).find((r) => r.dataset['key'] === g.key)!;
      group.focus();
      press(group, 'ArrowRight');
      await settle(f);
      expect((document.activeElement as HTMLElement).getAttribute('aria-level')).toBe('2');
      press(document.activeElement!, 'ArrowLeft');
      await settle(f);
      expect(document.activeElement).toBe(rowEls(el).find((r) => r.dataset['key'] === g.key));
    });
    it('Enter and Space show details of a process', async () => {
      const { f, el, store } = await make();
      store.setMode('flat');
      f.detectChanges();
      await settle(f);
      const select = vi.spyOn(store, 'select');
      const first = rowEls(el).find((r) => !r.hasAttribute('aria-expanded'))!;
      first.focus();
      press(first, 'Enter');
      press(first, ' ');
      expect(select).toHaveBeenCalledTimes(2);
    });
    it('Escape closes the details panel and keeps the focus on the row', async () => {
      const { f, el, store } = await make();
      store.setMode('flat');
      f.detectChanges();
      await settle(f);
      const first = rowEls(el).find((r) => !r.hasAttribute('aria-expanded'))!;
      first.focus();
      press(first, 'Enter');
      await settle(f);
      expect(store.selected()).not.toBeNull();
      press(rowEls(el).find((r) => r.dataset['key'] === first.dataset['key'])!, 'Escape');
      await settle(f);
      expect(store.selected()).toBeNull();
      // Nothing open: Escape is left to others.
      expect(press(rowEls(el)[0]!, 'Escape')).toBe(true);
    });
    it('opens the row menu with Shift+F10 and offers the actions of a real process', async () => {
      const { f, el, store } = await make();
      store.setMode('flat');
      f.detectChanges();
      await settle(f);
      const first = rowEls(el)[0]!;
      first.focus();
      first.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, cancelable: true, bubbles: true }));
      await settle(f);
      // The overlay opens on a later turn than the key: wait for the menu, not for a number of turns (rule 49).
      await vi.waitFor(() => expect(document.querySelectorAll('.pd-popmenu__item').length).toBeGreaterThan(0));
      const items = [...document.querySelectorAll('.pd-popmenu__item')].map((i) => i.textContent!.trim());
      expect(items).toEqual(['Details', 'Open file location', 'Search the web…', 'Search file hash on VirusTotal…']);
      document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = ''));
    });
  });

  describe('row actions', () => {
    it('open the file location, search and select by the action chosen', async () => {
      const { c, store, processRow } = await make();
      const lookups = TestBed.inject(Lookups);
      const web = vi.spyOn(lookups, 'searchWeb').mockResolvedValue(false);
      const hash = vi.spyOn(lookups, 'searchHash').mockResolvedValue(false);
      const location = vi.spyOn(store, 'openLocation').mockResolvedValue();
      const select = vi.spyOn(store, 'select');
      const row = processRow();
      c.run({ id: 'details', row });
      c.run({ id: 'location', row });
      c.run({ id: 'web', row });
      c.run({ id: 'hash', row });
      expect(select).toHaveBeenCalledWith(row.ref);
      expect(location).toHaveBeenCalledWith(row.ref);
      expect(web).toHaveBeenCalled();
      expect(hash).toHaveBeenCalled();
      expect(c.actions({ ...row, kind: 'other' })).toEqual([]);
      expect(c.actions({ ...row, ref: null }).map((a: { id: string }) => a.id)).toEqual(['web']);
    });
  });

  describe('right-click search', () => {
    it('searches the web for the name, and the file hash only for a real process', async () => {
      const { c, processRow } = await make();
      const lookups = TestBed.inject(Lookups);
      const web = vi.spyOn(lookups, 'searchWeb').mockResolvedValue(false);
      const hash = vi.spyOn(lookups, 'searchHash').mockResolvedValue(false);
      const row = processRow();
      c.searchWeb(row);
      expect(web).toHaveBeenCalledWith(row.name, row.name);
      c.searchHash(row);
      expect(hash).toHaveBeenCalledWith({ process: row.ref }, row.name);
      c.searchHash({ ...row, ref: null });
      expect(hash).toHaveBeenCalledTimes(1);
    });
  });
});
