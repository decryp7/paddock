import { TestBed } from '@angular/core/testing';
import { of } from 'rxjs';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Dialogs } from '../../shared/dialog';
import { provideIcons } from '../../shared/icons';
import { LiveStore } from '../../state/live.store';
import { Lookups } from '../lookup/lookups';
import { ProcessesStore } from '../../state/processes.store';
import { AffinityDialog, CapDialog, LockersDialog } from './process-dialogs';
import type { Row } from './process-rows';
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
  await new Promise((r) => setTimeout(r, 5));
  f.detectChanges();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const c = f.componentInstance as any;
  const dialogs = TestBed.inject(Dialogs);
  const act = vi.spyOn(store, 'act');
  const processRow = () => {
    store.setMode('flat');
    return store.rows().find((r) => r.kind === 'process' && r.protectedReason === null)!;
  };
  return { f, store, mock, c, dialogs, act, el: f.nativeElement as HTMLElement, processRow };
}

describe('ProcessesPage actions', () => {
  it('summarises the machine', async () => {
    const { c } = await make();
    expect(c.summary()).toMatch(/processes · .*% CPU · .* memory in use/);
    expect(c.ariaSort('cpu')).toBe('descending');
    expect(c.ariaSort('name')).toBeNull();
    c.store.sortBy('cpu');
    expect(c.ariaSort('cpu')).toBe('ascending');
    expect(c.pid({ pid: null })).toBe('');
    expect(c.pid({ pid: 7 })).toBe('7');
    expect(c.prioLabel('normal')).toBeTruthy();
  });

  it('end confirms with the right wording and acts only when confirmed', async () => {
    const { c, dialogs, act, store } = await make();
    const confirm = vi.spyOn(dialogs, 'confirm');
    store.setMode('flat');
    const proc = store.rows().find((r) => r.kind === 'process')!;
    confirm.mockResolvedValueOnce(false);
    await c.end(proc);
    expect(confirm).toHaveBeenLastCalledWith(expect.objectContaining({ title: `End ${proc.name}?`, confirmLabel: 'End', danger: true }));
    expect(act).not.toHaveBeenCalled();
    confirm.mockResolvedValueOnce(true);
    await c.end(proc);
    expect(act).toHaveBeenCalledWith(proc, 'end');
    store.setMode('groups');
    const group = store.rows().find((r) => r.kind === 'group')!;
    confirm.mockResolvedValueOnce(false);
    await c.end(group);
    expect(confirm).toHaveBeenLastCalledWith(expect.objectContaining({ title: `End all of ${group.name}?`, confirmLabel: 'End all' }));
    expect(confirm.mock.calls.at(-1)![0].message).toContain('This ends');
  });

  it('affinity opens the dialog with the cores and applies the chosen mask', async () => {
    const { c, dialogs, act, processRow } = await make();
    const open = vi.spyOn(dialogs, 'open').mockReturnValue({ closed: of(0b11) } as never);
    const row: Row = processRow();
    await c.affinity(row);
    expect(open).toHaveBeenCalledWith(AffinityDialog, { name: row.name, cores: expect.any(Number), mask: null });
    expect(act).toHaveBeenCalledWith(row, { setAffinity: 3 });
  });
  it('affinity passes the known mask of the selected process and ignores a cancelled dialog', async () => {
    const { c, dialogs, act, processRow, store } = await make();
    const row = processRow();
    await store.select(row.ref!);
    const open = vi.spyOn(dialogs, 'open').mockReturnValue({ closed: of(undefined) } as never);
    await c.affinity(row);
    expect(open.mock.calls[0]![1]).toMatchObject({ mask: store.details()!.affinity });
    expect(act).not.toHaveBeenCalled();
  });

  it('cap opens the dialog with the name and applies the percentage', async () => {
    const { c, dialogs, act, processRow } = await make();
    const row = processRow();
    const open = vi.spyOn(dialogs, 'open').mockReturnValueOnce({ closed: of(40) } as never).mockReturnValueOnce({ closed: of(undefined) } as never);
    await c.cap(row);
    expect(open).toHaveBeenCalledWith(CapDialog, row.name);
    expect(act).toHaveBeenCalledWith(row, { setCpuCap: 40 });
    await c.cap(row);
    expect(act).toHaveBeenCalledTimes(1);
  });

  it('lockers opens the lockers dialog; details selects a process; groups are ignored', async () => {
    const { c, dialogs, store, processRow } = await make();
    const open = vi.spyOn(dialogs, 'open').mockReturnValue({ closed: of(undefined) } as never);
    c.lockers();
    expect(open).toHaveBeenCalledWith(LockersDialog);
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
  });

  it('shows a failed action inline and Dismiss clears it', async () => {
    const { f, el, store, c, processRow } = await make('windows', new Set(['act']));
    await c.store.act(processRow(), 'suspend');
    f.detectChanges();
    expect(el.querySelector('.banner[role=alert]')).not.toBeNull();
    el.querySelector<HTMLButtonElement>('.banner [aria-label=Dismiss]')!.click();
    f.detectChanges();
    expect(store.actionError()).toBeNull();
    expect(el.querySelector('.banner')).toBeNull();
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
    function rows(store: InstanceType<typeof ProcessesStore>, n = 3) {
      const host = document.createElement('div');
      const els = store.rows().slice(0, n).map((r) => {
        const d = document.createElement('div');
        d.setAttribute('role', 'row');
        d.tabIndex = 0;
        d.dataset['key'] = r.key;
        host.appendChild(d);
        return d;
      });
      document.body.appendChild(host);
      return { host, els };
    }
    const press = (c: { key(e: KeyboardEvent): void }, target: Element | null, key: string) => {
      const ev = new KeyboardEvent('keydown', { key, cancelable: true, bubbles: true });
      Object.defineProperty(ev, 'target', { value: target });
      c.key(ev);
      return ev;
    };

    it('moves focus with the arrow keys', async () => {
      const { c, store } = await make();
      const { host, els } = rows(store);
      expect(press(c, els[0]!, 'ArrowDown').defaultPrevented).toBe(true);
      expect(document.activeElement).toBe(els[1]);
      press(c, els[1]!, 'ArrowUp');
      expect(document.activeElement).toBe(els[0]);
      expect(press(c, els[0]!, 'ArrowUp').defaultPrevented).toBe(false);
      host.remove();
    });
    it('ignores keys outside a row or on an unknown row', async () => {
      const { c, store } = await make();
      const sel = vi.spyOn(store, 'select');
      expect(press(c, document.body, 'Enter').defaultPrevented).toBe(false);
      expect(press(c, null, 'Enter').defaultPrevented).toBe(false);
      const ghost = document.createElement('div');
      ghost.setAttribute('role', 'row');
      ghost.dataset['key'] = 'nope';
      expect(press(c, ghost, 'Enter').defaultPrevented).toBe(false);
      expect(sel).not.toHaveBeenCalled();
    });
    it('Right expands and Left collapses an expandable row', async () => {
      const { c, store } = await make();
      store.setMode('groups');
      const group = store.rows().find((r) => r.expandable)!;
      const el = document.createElement('div');
      el.setAttribute('role', 'row');
      el.dataset['key'] = group.key;
      const toggle = vi.spyOn(store, 'toggle');
      press(c, el, 'ArrowRight');
      if (!group.expanded) expect(toggle).toHaveBeenCalledTimes(1);
      press(c, el, 'ArrowLeft');
      if (group.expanded) expect(toggle).toHaveBeenCalledTimes(1);
      // leaf rows never toggle
      const leaf = store.rows().find((r) => !r.expandable)!;
      el.dataset['key'] = leaf.key;
      toggle.mockClear();
      press(c, el, 'ArrowRight');
      press(c, el, 'ArrowLeft');
      expect(toggle).not.toHaveBeenCalled();
    });
    it('Left collapses an expanded row', async () => {
      const { c, store } = await make();
      store.setMode('groups');
      const group = store.rows().find((r) => r.expandable)!;
      if (!group.expanded) store.toggle(group);
      const el = document.createElement('div');
      el.setAttribute('role', 'row');
      el.dataset['key'] = group.key;
      const toggle = vi.spyOn(store, 'toggle');
      press(c, el, 'ArrowLeft');
      expect(toggle).toHaveBeenCalledTimes(1);
      press(c, el, 'ArrowRight');
      expect(toggle).toHaveBeenCalledTimes(2);
    });
    it('Enter shows details; Delete ends unless protected', async () => {
      const { c, store, dialogs } = await make();
      const confirm = vi.spyOn(dialogs, 'confirm').mockResolvedValue(false);
      store.setMode('flat');
      const proc = store.rows().find((r) => r.kind === 'process' && !r.protectedReason)!;
      const prot = store.rows().find((r) => r.protectedReason)!;
      const el = document.createElement('div');
      el.setAttribute('role', 'row');
      el.dataset['key'] = proc.key;
      const select = vi.spyOn(store, 'select');
      expect(press(c, el, 'Enter').defaultPrevented).toBe(true);
      expect(select).toHaveBeenCalled();
      press(c, el, 'Delete');
      expect(confirm).toHaveBeenCalledTimes(1);
      el.dataset['key'] = prot.key;
      press(c, el, 'Delete');
      expect(confirm).toHaveBeenCalledTimes(1);
      press(c, el, 'x');
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
