import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { WhatsNew } from './dashboard/whats-new';
import { WatchDetailPage } from './watch-detail/watch-detail-page';
import { WatchesPage } from './watches/watches-page';
import type { WhatsNewItem } from '../core/api.types';
import { ClockService } from '../core/clock.service';
import { Clipboard } from '../shared/clipboard';
import { Confirm } from '../shared/ui/confirm-dialog';
import { WatchesStore } from '../state/watches.store';
import { appProviders, byText, flush, makeMock, NOW, settle, text } from '../../testing/test-helpers';

@Component({ template: '' })
class Stub {}
const routes = [
  { path: 'watches', component: Stub },
  { path: 'watches/:id', component: Stub },
  { path: 'watches/:id/edit', component: Stub },
];

const overlayItems = () => Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container [cdkMenuItem]'));
async function rightClick(f: { detectChanges(): void }, row: Element) {
  row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: 120, clientY: 80 }));
  f.detectChanges();
  await flush(10);
}
async function pickFromMenu(f: { detectChanges(): void }, label: string) {
  byText(document.body, '.cdk-overlay-container [cdkMenuItem]', label)!.click();
  f.detectChanges();
  await flush(10);
}

describe('right-click menus', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    document.querySelectorAll('.cdk-overlay-container').forEach((e) => (e.innerHTML = ''));
  });

  describe('the release feed', () => {
    const item: WhatsNewItem = {
      watchId: 'w-1',
      watchName: 'Acme Installer',
      sourceKind: 'folder',
      release: { version: '9.9.9', prerelease: false, name: 'n', location: 'https://x/y', publishedAtMs: null, firstSeenMs: 1, size: null, sha256: 'abc', product: null, channel: '', fields: {}, artifacts: [], updatedInPlace: false },
      releasedAtMs: NOW - 3_600_000,
      newerThanInstalled: false,
      unseen: true,
    };

    async function make() {
      const backend = makeMock();
      TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
      TestBed.inject(ClockService).set(NOW);
      TestBed.inject(WatchesStore).start();
      const f = TestBed.createComponent(WhatsNew);
      f.componentRef.setInput('items', [item]);
      await settle(f);
      return { f, backend, row: (f.nativeElement as HTMLElement).querySelector<HTMLElement>('li.item')! };
    }

    it('offers the same actions as the "..." button', async () => {
      const { f, row } = await make();
      await rightClick(f, row);
      const viaRightClick = overlayItems().map((i) => text(i));
      expect(viaRightClick).toEqual(['Open watch', 'Open release', 'Download', 'Mark as seen', 'Copy link', 'Copy SHA-256']);
      document.querySelectorAll('.cdk-overlay-container').forEach((e) => (e.innerHTML = ''));
      (row.querySelector('sp-menu-button button') as HTMLElement).click();
      f.detectChanges();
      await flush(10);
      expect(overlayItems().map((i) => text(i))).toEqual(viaRightClick);
    });

    it('runs the picked action on that row', async () => {
      const { f, backend, row } = await make();
      const copy = vi.spyOn(TestBed.inject(Clipboard), 'copy').mockResolvedValue(undefined as never);
      await rightClick(f, row);
      await pickFromMenu(f, 'Copy link');
      expect(copy).toHaveBeenCalledWith('https://x/y', 'Link');
      await rightClick(f, row);
      await pickFromMenu(f, 'Open release');
      await rightClick(f, row);
      await pickFromMenu(f, 'Download');
      await rightClick(f, row);
      await pickFromMenu(f, 'Mark as seen');
      expect(backend.calls).toEqual(expect.arrayContaining(['openRelease', 'downloadRelease', 'acknowledge']));
      const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
      await rightClick(f, row);
      await pickFromMenu(f, 'Open watch');
      expect(nav).toHaveBeenCalledWith(['/watches', 'w-1']);
    });

    it('gives the focus back to the row when the menu closes with Esc', async () => {
      const { f, row } = await make();
      row.focus();
      await rightClick(f, row);
      expect(overlayItems().length).toBeGreaterThan(0);
      document.querySelector('.cdk-overlay-container [cdkMenu]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true }));
      f.detectChanges();
      await flush(30);
      expect(overlayItems()).toHaveLength(0);
      expect(document.activeElement).toBe(row);
    });

    it('does not open the watch itself, and is a tab stop only once', async () => {
      const { f, row } = await make();
      const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
      await rightClick(f, row);
      expect(nav).not.toHaveBeenCalled();
      expect(row.getAttribute('tabindex')).toBe('0');
      row.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
      expect(nav).toHaveBeenCalledTimes(1);
    });
  });

  describe('the watch table', () => {
    async function make() {
      const backend = makeMock();
      TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
      const f = TestBed.createComponent(WatchesPage);
      TestBed.inject(WatchesStore).start();
      await settle(f);
      const rows = () => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('tbody tr'));
      return { f, backend, rows };
    }

    it('opens the row actions, with Delete last, separated and dangerous', async () => {
      const { f, rows } = await make();
      await rightClick(f, rows()[0]!);
      const items = overlayItems();
      expect(items.map((i) => text(i))).toEqual(['Open details', 'Check now', 'Open latest release', 'Mark as seen', 'Pause', 'Copy address', 'Edit', 'Delete watch']);
      expect(items.at(-1)!.classList.contains('danger')).toBe(true);
    });

    it('runs Check now, Edit and Copy address for the row that was clicked', async () => {
      const { f, backend, rows } = await make();
      const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
      const copy = vi.spyOn(TestBed.inject(Clipboard), 'copy').mockResolvedValue(undefined as never);
      const second = rows()[1]!;
      const name = text(second.querySelector('.name'));
      await rightClick(f, second);
      await pickFromMenu(f, 'Edit');
      expect(nav).toHaveBeenCalledWith(['/watches', expect.stringMatching(/^w-/), 'edit']);
      await rightClick(f, second);
      await pickFromMenu(f, 'Copy address');
      expect(copy).toHaveBeenCalledWith(expect.any(String), 'Address');
      await rightClick(f, second);
      await pickFromMenu(f, 'Check now');
      expect(backend.calls).toContain('checkNow');
      expect(name).not.toBe('');
    });

    it('asks before deleting, naming the watch, and deletes only when confirmed', async () => {
      const { f, backend, rows } = await make();
      const ask = vi.spyOn(TestBed.inject(Confirm), 'ask').mockResolvedValueOnce(false).mockResolvedValueOnce(true);
      const before = rows().length;
      const name = text(rows()[0]!.querySelector('.name'));
      await rightClick(f, rows()[0]!);
      await pickFromMenu(f, 'Delete watch');
      expect(ask).toHaveBeenCalledWith(expect.objectContaining({ title: `Delete "${name}"?`, danger: true }));
      expect(rows()).toHaveLength(before);
      expect(backend.calls).not.toContain('deleteWatch');
      await rightClick(f, rows()[0]!);
      await pickFromMenu(f, 'Delete watch');
      await settle(f);
      expect(backend.calls).toContain('deleteWatch');
      expect(rows()).toHaveLength(before - 1);
    });

    it('moves between rows with the arrow keys and opens one with Enter', async () => {
      const { f, rows } = await make();
      const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
      const key = (el: Element, k: string, keyCode: number) => el.dispatchEvent(new KeyboardEvent('keydown', { key: k, keyCode, bubbles: true, cancelable: true }));
      rows()[0]!.focus();
      key(rows()[0]!, 'ArrowDown', 40);
      expect(document.activeElement).toBe(rows()[1]);
      key(rows()[1]!, 'End', 35);
      expect(document.activeElement).toBe(rows().at(-1));
      key(document.activeElement!, 'Enter', 13);
      expect(nav).toHaveBeenCalledTimes(1);
      f.detectChanges();
    });

    it('shows the schedule in words next to when the next check is', async () => {
      const { rows, backend } = await make();
      const first = rows()[0]!;
      const summary = (await backend.listWatches()).find((w) => w.name === text(first.querySelector('.name')))!;
      const next = first.querySelector('.next')!;
      expect(text(next.querySelector('.sched'))).toBe(summary.scheduleText);
      expect(next.querySelector('.sched')!.getAttribute('title')).toBe(summary.scheduleText);
      expect(summary.scheduleText).not.toBe('');
      expect(text(next)).toContain(summary.scheduleText);
    });

    it('keeps the header in view while the table scrolls', async () => {
      const { f } = await make();
      expect((f.nativeElement as HTMLElement).querySelector('table.sticky')).not.toBeNull();
    });
  });

  describe('the detail tables', () => {
    async function make() {
      const backend = makeMock();
      TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
      TestBed.inject(WatchesStore).start();
      const f = TestBed.createComponent(WatchDetailPage);
      f.componentRef.setInput('id', 'w-1');
      await settle(f);
      return { f, backend };
    }
    const releaseRows = (f: { nativeElement: HTMLElement }) => Array.from(f.nativeElement.querySelectorAll<HTMLElement>('section[aria-labelledby=hist-h] tbody tr'));
    const productRows = (f: { nativeElement: HTMLElement }) => Array.from(f.nativeElement.querySelectorAll<HTMLElement>('section[aria-labelledby=prod-h] tbody tr'));

    it('has sticky headers on the release and product tables', async () => {
      const { f } = await make();
      expect(f.nativeElement.querySelectorAll('table.sticky').length).toBeGreaterThanOrEqual(1);
      expect(f.nativeElement.querySelector('section[aria-labelledby=hist-h] table.sticky')).not.toBeNull();
    });

    it('opens the release actions on a right click and acts on that release', async () => {
      const { f, backend } = await make();
      const r = (await backend.getWatch('w-1')).history[1]!;
      const row = releaseRows(f)[1]!;
      await rightClick(f, row);
      expect(overlayItems().map((i) => text(i)).slice(0, 4)).toEqual(['Show details', 'Open release', 'Download', 'Copy link']);
      await pickFromMenu(f, 'Show details');
      await settle(f);
      expect(f.nativeElement.querySelector('aside.panel')).not.toBeNull();
      expect(text(f.nativeElement.querySelector('aside.panel h2'))).toBe(r.version);
      await rightClick(f, row);
      await pickFromMenu(f, 'Download');
      await rightClick(f, row);
      await pickFromMenu(f, 'Open release');
      expect(backend.calls).toEqual(expect.arrayContaining(['downloadRelease', 'openRelease']));
    });

    it('selects a release with Enter and walks the rows with the arrows', async () => {
      const { f } = await make();
      const rows = releaseRows(f);
      rows[0]!.focus();
      rows[0]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', keyCode: 40, bubbles: true, cancelable: true }));
      expect(document.activeElement).toBe(rows[1]);
      rows[1]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', keyCode: 13, bubbles: true, cancelable: true }));
      await settle(f);
      expect(f.nativeElement.querySelector('aside.panel')).not.toBeNull();
    });

    it('offers the product actions and marks a product seen', async () => {
      const backend = makeMock({ scenario: 'products' });
      TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
      TestBed.inject(WatchesStore).start();
      const withProducts = (await backend.listWatches()).find((w) => w.productCount > 0)!;
      const f = TestBed.createComponent(WatchDetailPage);
      f.componentRef.setInput('id', withProducts.id);
      await settle(f);
      const rows = productRows(f);
      expect(rows.length).toBeGreaterThan(0);
      await rightClick(f, rows[0]!);
      expect(overlayItems().map((i) => text(i))).toEqual(['Show files', 'Mark as seen']);
      await pickFromMenu(f, 'Show files');
      await settle(f);
      expect(f.nativeElement.querySelector('aside.panel')).not.toBeNull();
      const unseen = (await backend.getWatch(withProducts.id)).products.findIndex((p) => p.unseen);
      if (unseen >= 0) {
        await rightClick(f, productRows(f)[unseen]!);
        await pickFromMenu(f, 'Mark as seen');
        expect(backend.calls).toContain('acknowledgeProduct');
      }
    });
  });
});
