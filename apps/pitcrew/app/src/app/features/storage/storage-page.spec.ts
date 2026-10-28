import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { appProviders, byText, flush, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';
import { StatusStore } from '../../state/status.store';
import { StoragePage } from './storage-page';

describe('StoragePage', () => {
  let mock: MockBackend;
  beforeEach(() => {
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
  });
  afterEach(() => {
    document.querySelectorAll('.cdk-overlay-container > *').forEach((n) => n.remove());
    mock.dispose();
  });

  /** No runner has a job, so a prune or clean is allowed. */
  const allIdle = () => (mock as unknown as { runners: { currentJob: unknown }[] }).runners.forEach((r) => (r.currentJob = null));

  async function mount() {
    await TestBed.inject(StatusStore).load();
    const f = TestBed.createComponent(StoragePage);
    await settle(f, 6);
    return { f, el: f.nativeElement as HTMLElement };
  }
  const click = async (f: { detectChanges(): void }, el: Element | null) => {
    (el as HTMLElement).click();
    await flush();
    f.detectChanges();
    await flush();
    f.detectChanges();
  };
  const dialog = () => document.querySelector('[role="dialog"]') as HTMLElement | null;
  const btn = (el: HTMLElement, testid: string) => el.querySelector<HTMLButtonElement>(`[data-testid="${testid}"]`)!;
  const dlgButton = (label: string) => byText(dialog()!, 'button', label)!;

  describe('container engine', () => {
    it('shows a card per usage row with words and the engine name', async () => {
      const { el } = await mount();
      expect(text(el.querySelector('[data-testid="engine-name"]'))).toBe('Docker');
      const images = el.querySelector('[data-testid="usage-Images"]')!;
      expect(text(images)).toContain('18.4 GB');
      expect(text(images)).toContain('34 in total, 6 active');
      expect(text(images)).toContain('12.1 GB');
      expect(text(images)).toContain('12.1 GB reclaimable');
      expect(images.querySelectorAll('.stack').length).toBe(1);
      expect(text(images.querySelector('.legend'))).toBe('In useReclaimable');
      expect(el.querySelectorAll('.usage-card').length).toBe(4);
      expect(text(el)).toContain('Cache server (default)');
      expect(el.querySelector('table.folders .mono')!.textContent).toContain('act');
      expect([...el.querySelectorAll('table.folders thead th')].slice(0, 3).map((h) => text(h))).toEqual(['Folder', 'Size', 'Entries']);
    });

    it('has a subtitle, a refresh icon button and one primary that focuses the prune section', async () => {
      const { f, el } = await mount();
      expect(text(el.querySelector('.subtitle'))).toContain('reclaim');
      expect(btn(el, 'refresh').getAttribute('aria-label')).toBe('Refresh');
      expect(el.querySelectorAll('.btn.primary').length).toBe(1);
      const scroll = vi.fn();
      (document.getElementById('prune-title') as HTMLElement).scrollIntoView = scroll;
      await click(f, btn(el, 'clean-up'));
      expect(scroll).toHaveBeenCalled();
      expect(document.activeElement?.id).toBe('prune-title');
    });

    it('refreshes the report from the toolbar', async () => {
      const { f, el } = await mount();
      const spy = vi.spyOn(mock, 'engineReport');
      await click(f, btn(el, 'refresh'));
      expect(spy).toHaveBeenCalled();
    });

    it('copies a folder path and shortens long paths in the middle with the full path as tooltip', async () => {
      const write = vi.fn().mockResolvedValue(undefined);
      Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
      const { f, el } = await mount();
      const row = el.querySelector('table.folders tbody tr')!;
      const path = row.querySelector('.path')!;
      expect(path.getAttribute('title')).toBeTruthy();
      await click(f, row.querySelector('button[aria-label^="Copy path"]'));
      expect(write).toHaveBeenCalledWith(path.getAttribute('title'));
      expect(row.querySelector('button[aria-label^="More actions"]')).not.toBeNull();
    });

    it('shows the problem and a next step, with no prune controls, when there is no engine', async () => {
      const base = await mock.engineReport();
      vi.spyOn(mock, 'engineReport').mockResolvedValue({ ...base, engine: null, problem: 'docker was not found on PATH', usage: [] });
      const { el } = await mount();
      const p = text(el.querySelector('[data-testid="engine-problem"]'));
      expect(p).toContain('docker was not found on PATH');
      expect(p).toContain('Next step');
      expect(btn(el, 'preview')).toBeNull();
      expect(el.querySelector('[data-testid="engine-name"]')).toBeNull();
    });

    it('shows a load error with Retry', async () => {
      vi.spyOn(mock, 'engineReport').mockRejectedValueOnce({ code: 'serviceUnavailable', message: 'The service is not running.' });
      const { f, el } = await mount();
      const alert = el.querySelector('[role="alert"]')!;
      expect(text(alert)).toContain('The container engine report could not be loaded');
      await click(f, byText(alert, 'button', 'Retry'));
      await settle(f, 3);
      expect(el.querySelectorAll('.usage-card').length).toBe(4);
    });

    it('offers the five kinds with volumes off and warned about', async () => {
      const { el } = await mount();
      const boxes = Array.from(el.querySelectorAll<HTMLInputElement>('.kinds input'));
      expect(boxes.length).toBe(5);
      expect(el.querySelector<HTMLInputElement>('[data-kind="volumes"]')!.checked).toBe(false);
      expect(el.querySelector<HTMLInputElement>('[data-kind="buildCache"]')!.checked).toBe(true);
      expect(text(el.querySelector('.kinds'))).toContain('removes data; only unused volumes');
    });

    it('requires a preview of the same selection before Prune is enabled', async () => {
      const { f, el } = await mount();
      expect(btn(el, 'prune').disabled).toBe(true);
      await click(f, btn(el, 'preview'));
      expect(btn(el, 'prune').disabled).toBe(false);
      const preview = text(el.querySelector('[data-testid="engine-preview"]'));
      expect(preview).toContain('Unused images');
      expect(preview).toContain('would be freed');
      expect(preview).toContain('node:18-bookworm');
      await click(f, el.querySelector('[data-kind="volumes"]'));
      expect(btn(el, 'prune').disabled).toBe(true);
      expect(el.querySelector('[data-testid="engine-preview"]')).toBeNull();
      typeInto(el.querySelector<HTMLInputElement>('#older-than')!, '30');
      f.detectChanges();
      await click(f, btn(el, 'preview'));
      expect(btn(el, 'prune').disabled).toBe(false);
    });

    it('does nothing when the confirmation is cancelled', async () => {
      const { f, el } = await mount();
      const prune = vi.spyOn(mock, 'pruneEngine');
      await click(f, btn(el, 'preview'));
      await click(f, btn(el, 'prune'));
      expect(dialog()).not.toBeNull();
      expect(text(dialog())).toContain('cannot be undone');
      await click(f, dlgButton('Cancel'));
      expect(prune).not.toHaveBeenCalled();
      expect(el.querySelector('[data-testid="engine-result"]')).toBeNull();
    });

    it('prunes after the confirmation, shows what was freed and reloads the report', async () => {
      allIdle();
      const { f, el } = await mount();
      const prune = vi.spyOn(mock, 'pruneEngine');
      const report = vi.spyOn(mock, 'engineReport');
      await click(f, btn(el, 'preview'));
      await click(f, btn(el, 'prune'));
      expect(text(dialog())).toContain('Remove');
      await click(f, dlgButton('Prune now'));
      await settle(f, 3);
      expect(prune).toHaveBeenCalledWith({ kinds: ['stoppedContainers', 'danglingImages', 'unusedImages', 'buildCache'], olderThanDays: 0 });
      expect(report).toHaveBeenCalled();
      expect(text(el.querySelector('[data-testid="engine-result"]'))).toContain('Freed');
      expect(text(el.querySelector('[data-testid="engine-result"]'))).toContain('Build cache');
    });

    it('lists failures with the engine’s message', async () => {
      const { f, el } = await mount();
      vi.spyOn(mock, 'pruneEngine').mockResolvedValueOnce({ freed: [['buildCache', 1000 * 1000 * 1000]], failures: [['volumes', 'volume pgdata is in use']], at: 0 });
      await click(f, el.querySelector('[data-kind="volumes"]'));
      await click(f, btn(el, 'preview'));
      await click(f, btn(el, 'prune'));
      expect(text(dialog())).toContain('Volumes hold data');
      await click(f, dlgButton('Prune now'));
      await settle(f, 3);
      const r = text(el.querySelector('[data-testid="engine-result"]'));
      expect(r).toContain('Volumes failed: volume pgdata is in use');
      expect(r).toContain('failed');
    });

    it('explains a busy engine in a sentence with Retry, which asks again', async () => {
      const { f, el } = await mount();
      vi.spyOn(mock, 'pruneEngine').mockRejectedValueOnce({ code: 'busy', message: 'Try again when it is idle.' });
      await click(f, btn(el, 'preview'));
      await click(f, btn(el, 'prune'));
      await click(f, dlgButton('Prune now'));
      await settle(f, 3);
      const err = el.querySelector('[data-testid="engine-action-error"]')!;
      expect(text(err)).toContain('A job is running, so nothing is removed right now.');
      await click(f, byText(err, 'button', 'Retry'));
      expect(dialog()).not.toBeNull();
      await click(f, dlgButton('Cancel'));
    });
  });

  describe('actions cache', () => {
    it('picks the first runner and shows its cache', async () => {
      const { el } = await mount();
      const pressed = el.querySelector('.segmented [aria-pressed="true"]')!;
      expect(text(pressed)).toBe('build-win');
      expect(text(el.querySelector('[data-testid="cache-enabled"]'))).toBe('Cache server: enabled');
      expect(el.querySelector('[data-testid="cache-path"]')!.textContent).toContain('cache');
      expect(text(el)).toContain('actions-cache-npm-4f2a');
      expect(text(el)).toContain('6 GB');
      expect(el.querySelector('[data-testid="cache-config-link"]')!.getAttribute('href')).toBe('/runners/build-win?tab=config');
    });

    it('switches runner', async () => {
      const { f, el } = await mount();
      const spy = vi.spyOn(mock, 'cacheReport');
      await click(f, byText(el, '.segmented button', 'docker-lab'));
      await settle(f, 3);
      expect(spy).toHaveBeenCalledWith('docker-lab');
      expect(text(el.querySelector('#cache-info-title'))).toBe('docker-lab');
    });

    it('warns in words when the cache is over its limit, and shows a problem line', async () => {
      const base = await mock.cacheReport('build-win');
      vi.spyOn(mock, 'cacheReport').mockResolvedValue({ ...base, overLimit: true, problem: 'cannot read part of the folder', defaultDir: true });
      const { el } = await mount();
      expect(text(el.querySelector('[data-testid="cache-over-limit"]'))).toContain('Over the size limit');
      expect(text(el.querySelector('[data-testid="cache-problem"]'))).toContain('cannot read part of the folder');
      expect(text(el)).toContain('Default location.');
    });

    it('needs a limit, then previews with reasons and cleans after a confirmation', async () => {
      allIdle();
      const { f, el } = await mount();
      const clean = vi.spyOn(mock, 'cleanCache');
      expect(btn(el, 'cache-preview').disabled).toBe(true);
      expect(el.querySelector('[data-testid="cache-limits-hint"]')).not.toBeNull();
      typeInto(el.querySelector<HTMLInputElement>('#keep-gb')!, '3');
      typeInto(el.querySelector<HTMLInputElement>('#older-days')!, '30');
      f.detectChanges();
      expect(btn(el, 'cache-clean').disabled).toBe(true);
      await click(f, btn(el, 'cache-preview'));
      const plan = text(el.querySelector('[data-testid="cache-plan"]'));
      expect(plan).toContain('older than limit');
      expect(plan).toContain('over the size limit');
      expect(plan).toContain('would be freed');
      typeInto(el.querySelector<HTMLInputElement>('#keep-gb')!, '4');
      f.detectChanges();
      expect(btn(el, 'cache-clean').disabled).toBe(true);
      await click(f, btn(el, 'cache-preview'));
      await click(f, btn(el, 'cache-clean'));
      await click(f, dlgButton('Cancel'));
      expect(clean).not.toHaveBeenCalled();
      await click(f, btn(el, 'cache-clean'));
      await click(f, dlgButton('Delete now'));
      await settle(f, 3);
      expect(clean).toHaveBeenCalledWith('build-win', { maxMb: 4096, maxAgeDays: 30 });
      expect(text(el.querySelector('[data-testid="cache-result"]'))).toContain('Deleted');
    });

    it('shows failures of a clean', async () => {
      allIdle();
      const { f, el } = await mount();
      const plan = await mock.previewCacheClean('build-win', { maxMb: 0, maxAgeDays: 1 });
      vi.spyOn(mock, 'cleanCache').mockResolvedValueOnce({ plan, failures: [['actions-cache-npm-4f2a', 'file is in use']] });
      typeInto(el.querySelector<HTMLInputElement>('#older-days')!, '1');
      f.detectChanges();
      await click(f, btn(el, 'cache-preview'));
      await click(f, btn(el, 'cache-clean'));
      await click(f, dlgButton('Delete now'));
      await settle(f, 3);
      expect(text(el.querySelector('[data-testid="cache-result"]'))).toContain('actions-cache-npm-4f2a was not deleted: file is in use');
    });

    it('explains a busy runner and an empty plan', async () => {
      const { f, el } = await mount();
      typeInto(el.querySelector<HTMLInputElement>('#older-days')!, '3650');
      f.detectChanges();
      await click(f, btn(el, 'cache-preview'));
      expect(text(el.querySelector('[data-testid="cache-plan"]'))).toContain('Nothing to delete');
      vi.spyOn(mock, 'previewCacheClean').mockRejectedValueOnce({ code: 'busy', message: 'Try again later.' });
      await click(f, btn(el, 'cache-preview'));
      expect(text(el.querySelector('[data-testid="cache-action-error"]'))).toContain('A job is running');
    });

    it('treats a field that is cleared or invalid as no limit', async () => {
      const { f, el } = await mount();
      const gb = el.querySelector<HTMLInputElement>('#keep-gb')!;
      typeInto(gb, '2');
      f.detectChanges();
      expect(btn(el, 'cache-preview').disabled).toBe(false);
      typeInto(gb, '');
      f.detectChanges();
      expect(btn(el, 'cache-preview').disabled).toBe(true);
      typeInto(gb, '-5');
      f.detectChanges();
      expect(btn(el, 'cache-preview').disabled).toBe(true);
    });
  });
});
