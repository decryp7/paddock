import { Component, input } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, provideRouter, Router } from '@angular/router';
import type { MaintenanceSettings, SleepReport } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Chart } from '../../shared/chart';
import { provideIcons } from '../../shared/icons';
import { ExtrasStore } from '../../state/extras.store';
import { SettingsStore } from '../../state/settings.store';
import { ExtrasBoot } from './extras-boot';
import { ExtrasPage, tabFrom } from './extras-page';

@Component({ selector: 'dy-chart', template: '' })
class StubChart {
  readonly options = input<unknown>(null);
  readonly label = input('');
}

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

function render(tab = '', options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, ...options });
  TestBed.configureTestingModule({
    providers: [
      provideIcons(),
      provideRouter([]),
      { provide: BACKEND, useValue: mock },
      { provide: ActivatedRoute, useValue: { snapshot: { queryParamMap: new Map(tab ? [['tab', tab]] : []) } } },
    ],
  });
  TestBed.overrideComponent(ExtrasBoot, { remove: { imports: [Chart] }, add: { imports: [StubChart] } });
  const f = TestBed.createComponent(ExtrasPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock, store: TestBed.inject(ExtrasStore) };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text))!;
const tabBtn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((b) => b.textContent === text)!;
const rowsOf = (el: HTMLElement, label: string) => el.querySelectorAll(`table[aria-label="${label}"] tbody tr`);
const clickTab = async (f: Parameters<typeof settle>[0], el: HTMLElement, text: string) => {
  tabBtn(el, text).click();
  await settle(f);
};

describe('dy-extras-page', () => {
  it('has the eight tabs in a tablist, opens What changed first, and reads each tab only when opened', async () => {
    const { f, el, mock } = render();
    const spy = vi.spyOn(mock, 'bootAnalysis');
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Extras');
    expect([...el.querySelectorAll('[role="tab"]')].map((b) => b.textContent)).toEqual(['What changed', 'Sleep', 'Boot', 'Apps', 'GPU', 'Files', 'Inventory', 'Maintenance']);
    expect(tabBtn(el, 'What changed').getAttribute('aria-selected')).toBe('true');
    expect(spy).not.toHaveBeenCalled();
    await clickTab(f, el, 'Boot');
    expect(spy).toHaveBeenCalledTimes(1);
    expect(tabBtn(el, 'Boot').getAttribute('aria-selected')).toBe('true');
    await clickTab(f, el, 'Boot');
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it('follows the tab named in the address, ignores a name it does not know, and moves with the tab links', async () => {
    expect(tabFrom('files')).toBe('files');
    expect(tabFrom('nope')).toBe('changes');
    expect(tabFrom(null)).toBe('changes');
    const { f, el } = render('gpu');
    await settle(f);
    expect(tabBtn(el, 'GPU').getAttribute('aria-selected')).toBe('true');
    await clickTab(f, el, 'Files');
    expect(tabBtn(el, 'Files').getAttribute('aria-selected')).toBe('true');
    await clickTab(f, el, 'Apps');
    expect(tabBtn(el, 'Apps').getAttribute('aria-selected')).toBe('true');
    expect(TestBed.inject(Router).url).toContain('tab=apps');
  });

  it('What changed: one table grouped by day, the window control and the watching note', async () => {
    const { f, el, store } = render();
    await settle(f);
    const table = el.querySelector('table[aria-label="What changed"]')!;
    expect(table.querySelectorAll('thead')).toHaveLength(1);
    expect(table.querySelectorAll('tr.day').length).toBeGreaterThanOrEqual(3);
    expect(table.querySelector('tr.day')!.textContent).toContain('Today');
    expect(table.textContent).toContain('Updated Visual Studio Code');
    expect(el.textContent).toContain('Dyno has been watching since');
    const before = rowsOf(el, 'What changed').length;
    btn(el, '7 days').click();
    await settle(f);
    expect(store.hours()).toBe(168);
    expect(rowsOf(el, 'What changed').length).toBeGreaterThan(before);
    btn(el, '24 hours').click();
    await settle(f);
    expect(rowsOf(el, 'What changed').length).toBeLessThan(before);
  });

  it('What changed: an empty period is a designed state', async () => {
    const { f, el, mock } = render();
    vi.spyOn(mock, 'changes').mockResolvedValue({ from: 0, to: NOW, changes: [], watchingSince: null });
    await TestBed.inject(ExtrasStore).reload('changes');
    await settle(f);
    expect(el.querySelector('.none strong')?.textContent).toBe('Nothing changed in this period');
  });

  it('Sleep: two tables, and the engine sentence in a designed state when a part is unavailable', async () => {
    const { f, el, mock } = render('sleep');
    await settle(f);
    expect(rowsOf(el, 'Keeping this computer awake').length).toBe(3);
    expect(el.textContent).toContain('Video call in progress');
    expect(el.textContent).toContain('42 min 17 s');
    expect(rowsOf(el, 'Recent wakes').length).toBe(5);
    const report: SleepReport = { blockers: [], wakes: [], blockersUnavailable: 'pmset is not available here.', wakesUnavailable: null };
    vi.spyOn(mock, 'sleepBlockers').mockResolvedValue(report);
    await TestBed.inject(ExtrasStore).reload('sleep');
    await settle(f);
    expect(el.querySelector('.none')!.textContent).toContain('pmset is not available here.');
    expect(el.textContent).toContain('No wakes recorded');
    vi.spyOn(mock, 'sleepBlockers').mockResolvedValue({ ...report, blockersUnavailable: null, wakesUnavailable: 'No log.' });
    await TestBed.inject(ExtrasStore).reload('sleep');
    await settle(f);
    expect(el.textContent).toContain('Nothing is keeping this computer awake');
    expect(el.textContent).toContain('No log.');
  });

  it('Boot: last start-up, verdict, what slowed it and a note when steps are unavailable', async () => {
    const { f, el, mock, store } = render('boot');
    await settle(f);
    expect(el.querySelector('[aria-label="Last start-up"] .big')?.textContent).toBe('47.2 s');
    expect(el.textContent).toContain('about the same as usual');
    expect(el.querySelector('dy-chart')).not.toBeNull();
    expect(rowsOf(el, 'What slowed start-up').length).toBe(5);
    const real = await mock.bootAnalysis();
    vi.spyOn(mock, 'bootAnalysis').mockResolvedValue({ ...real, slowest: [], stepsUnavailable: 'Windows has no boot trace here.' });
    await store.reload('boot');
    await settle(f);
    expect(el.querySelector('p.note')!.textContent).toContain('Windows has no boot trace here.');
    vi.spyOn(mock, 'bootAnalysis').mockResolvedValue({ ...real, slowest: [], stepsUnavailable: null, boots: real.boots.slice(0, 1) });
    await store.reload('boot');
    await settle(f);
    expect(el.textContent).toContain('Nothing slowed the last start-up');
    expect(el.querySelector('dy-chart')).toBeNull();
    vi.spyOn(mock, 'bootAnalysis').mockResolvedValue({ boots: [], slowest: [], lastTotalSecs: null, verdict: null, stepsUnavailable: 'No history.' });
    await store.reload('boot');
    await settle(f);
    expect(el.querySelector('.none')!.textContent).toContain('No history.');
    vi.spyOn(mock, 'bootAnalysis').mockResolvedValue({ ...real, stepsUnavailable: 'Only the top ones.' });
    await store.reload('boot');
    await settle(f);
    expect(el.textContent).toContain('Only the top ones.');
  });

  it('Apps: sortable, units from the formatters, notes when a part is unavailable', async () => {
    const { f, el, mock, store } = render('apps');
    await settle(f);
    const names = () => [...rowsOf(el, 'Network and energy per app')].map((r) => r.querySelector('.nm')!.textContent);
    expect(names()[0]).toBe('Google Chrome');
    expect(el.textContent).toContain('4.8 GB');
    btn(el, 'Sent').click();
    await settle(f);
    expect(names()[0]).toBe('Dropbox');
    expect(el.querySelector('th[aria-sort="descending"]')!.textContent).toContain('Sent');
    btn(el, 'Sent').click();
    await settle(f);
    expect(el.querySelector('th[aria-sort="ascending"]')).not.toBeNull();
    [...el.querySelectorAll<HTMLButtonElement>('.sort')].find((b) => b.textContent!.startsWith('App'))!.click();
    await settle(f);
    expect(names()[0]).toBe('Docker Desktop');
    const real = await mock.appUsage();
    vi.spyOn(mock, 'appUsage').mockResolvedValue({ ...real, networkUnavailable: 'Needs nettop.', energyUnavailable: 'No energy data.' });
    await store.reload('apps');
    await settle(f);
    expect(el.textContent).toContain('Needs nettop.');
    expect(el.textContent).toContain('No energy data.');
    expect(btn(el, 'Received')).toBeUndefined();
    vi.spyOn(mock, 'appUsage').mockResolvedValue({ apps: [], networkUnavailable: null, energyUnavailable: null });
    await store.reload('apps');
    await settle(f);
    expect(el.textContent).toContain('No app activity to show');
  });

  it('GPU: a table, and the unavailable and idle states', async () => {
    const { f, el, mock, store } = render('gpu');
    await settle(f);
    expect(rowsOf(el, 'GPU use per process').length).toBe(5);
    expect(el.textContent).toContain('WindowServer');
    expect(el.textContent).toContain('From Apple GPU accelerator counters.');
    vi.spyOn(mock, 'gpuProcesses').mockResolvedValue({ processes: [], source: null, unavailable: 'No GPU counters.' });
    await store.reload('gpu');
    await settle(f);
    expect(el.querySelector('.none')!.textContent).toContain('No GPU counters.');
    vi.spyOn(mock, 'gpuProcesses').mockResolvedValue({ processes: [], source: null, unavailable: null });
    await store.reload('gpu');
    await settle(f);
    expect(el.querySelector('.none strong')!.textContent).toContain('No process is using the GPU');
  });

  it('Inventory: searches, exports through a menu, shows where it was saved and its states', async () => {
    const { f, el, mock, store } = render('inventory');
    await settle(f);
    expect(rowsOf(el, 'Installed software').length).toBeGreaterThanOrEqual(41);
    const search = el.querySelector<HTMLInputElement>('input[type="search"]')!;
    search.value = 'apple';
    search.dispatchEvent(new Event('input'));
    await settle(f);
    expect(rowsOf(el, 'Installed software').length).toBeGreaterThan(5);
    expect(el.textContent).toMatch(/\d+ of 41 programs/);
    search.value = 'zzzz';
    search.dispatchEvent(new Event('input'));
    await settle(f);
    expect(el.querySelector('.none strong')!.textContent).toContain('Nothing matches');
    search.value = '';
    search.dispatchEvent(new Event('input'));
    btn(el, 'Export report').click();
    await settle(f);
    const items = [...document.querySelectorAll<HTMLButtonElement>('[role="menu"] button')];
    expect(items.map((i) => i.textContent!.trim())).toEqual(['Markdown (readable)', 'JSON (for other tools)']);
    items[0]!.click();
    await settle(f);
    expect(mock.savedInventories).toEqual(['markdown']);
    expect(el.querySelector('[role="status"]')!.textContent).toContain('dyno-inventory.md');
    const real = await mock.softwareInventory();
    vi.spyOn(mock, 'softwareInventory').mockResolvedValue({ ...real, items: real.items.slice(0, 2), truncated: true });
    await store.reload('inventory');
    await settle(f);
    expect(el.textContent).toContain('more packages than Dyno lists');
    vi.spyOn(mock, 'softwareInventory').mockResolvedValue({ items: [], truncated: false, unavailable: 'No package manager.' });
    await store.reload('inventory');
    await settle(f);
    expect(el.querySelector('.none')!.textContent).toContain('No package manager.');
    vi.spyOn(mock, 'softwareInventory').mockResolvedValue({ items: [], truncated: false, unavailable: null });
    await store.reload('inventory');
    await settle(f);
    expect(el.querySelector('.none strong')!.textContent).toBe('No software found');
    vi.spyOn(mock, 'saveInventoryReport').mockRejectedValueOnce({ code: 'accessDenied', message: 'Read-only folder.' });
    await store.exportReport('json');
    await settle(f);
    expect(el.querySelector('.note.bad')!.textContent).toContain('Read-only folder.');
  });

  it('every tab shows an inline error with Retry', async () => {
    const fails: [string, string][] = [['What changed', 'changes'], ['Sleep', 'sleepBlockers'], ['Boot', 'bootAnalysis'], ['Apps', 'appUsage'], ['GPU', 'gpuProcesses'], ['Inventory', 'softwareInventory']];
    const { f, el, mock } = render('', { fail: new Set(fails.map((x) => x[1])) });
    await settle(f);
    for (const [tab] of fails) {
      await clickTab(f, el, tab);
      expect(el.querySelector('[role="alert"]'), tab).not.toBeNull();
      expect(btn(el, 'Retry'), tab).toBeTruthy();
    }
    expect(mock).toBeTruthy();
  });

  it('Files: starts with a designed state and one primary button, then lists large files and duplicate groups in one table', async () => {
    const { f, el, mock } = render('files');
    await settle(f);
    expect(el.querySelector('.none strong')!.textContent).toBe('Find big and duplicate files');
    expect(el.querySelectorAll('button[matButton="filled"]')).toHaveLength(1);
    btn(el, 'Choose a folder').click();
    await settle(f);
    expect(mock.pickedFolders).toBe(1);
    const table = el.querySelector('table[aria-label="Large and duplicate files"]')!;
    expect(table.querySelectorAll('thead')).toHaveLength(1);
    expect([...table.querySelectorAll('tr.day strong')].map((s) => s.textContent)).toEqual(['Large files', '3 copies of site-redesign.psd', '2 copies of resnet50.onnx']);
    expect(table.querySelectorAll('input[type="checkbox"]')).toHaveLength(0);
    expect(table.querySelectorAll('tbody tr:not(.day)')).toHaveLength(8 + 5);
    expect(el.textContent).toContain('Copies are listed oldest first.');
    expect(btn(el, 'Choose another folder')).toBeTruthy();
  });

  it('Files: filters, collapse and the empty result work', async () => {
    const { f, el, mock, store } = render('files');
    await settle(f);
    btn(el, 'Choose a folder').click();
    await settle(f);
    (el.querySelector('[aria-label="Collapse Large files"]') as HTMLElement).click();
    await settle(f);
    expect(el.querySelectorAll('table[aria-label="Large and duplicate files"] tbody tr:not(.day)').length).toBe(5);
    btn(el, 'Duplicates').click();
    await settle(f);
    expect(store.mode()).toBe('duplicates');
    btn(el, 'Large').click();
    await settle(f);
    btn(el, '1 GB').click();
    await settle(f);
    expect(store.minMib()).toBe(1024);
    expect(el.querySelector('.none strong')).toBeNull();
    vi.spyOn(mock, 'findFiles').mockResolvedValue({ scanId: 's', folder: '/x', large: [], duplicates: [], scanned: 10, truncated: true, wastedBytes: 0 });
    await store.search();
    await settle(f);
    expect(el.querySelector('.none strong')!.textContent).toBe('Nothing found in this folder');
    expect(el.textContent).toContain('before the limit');
    vi.spyOn(mock, 'findFiles').mockRejectedValue({ code: 'badRequest', message: 'Choose a folder with a full path.' });
    await store.search();
    await settle(f);
    expect(el.querySelector('[role="alert"]')!.textContent).toContain('Choose a folder with a full path.');
  });

  describe('Maintenance', () => {
    it('explains what it limits, shows the status and the windows', async () => {
      const { f, el } = render('maintenance');
      await settle(f);
      expect(el.textContent).toContain('never limits anything you click');
      expect(el.querySelector('.status')!.textContent).toContain('Closed now. Next window opens');
      expect(el.querySelectorAll('li.win')).toHaveLength(2);
      expect(el.querySelector<HTMLInputElement>('input[aria-label="From, window 1"]')!.value).toBe('22:00');
      expect(el.querySelector<HTMLInputElement>('input[aria-label="To, window 1"]')!.value).toBe('02:00');
      expect(el.textContent).toContain('Mon to Fri, 22:00 to 02:00 the next day');
      expect(el.querySelectorAll('button[matButton="filled"]')).toHaveLength(1);
      expect(btn(el, 'Save windows').disabled).toBe(true);
    });

    it('edits days, times and windows, validates early, and saves through the settings', async () => {
      const { f, el, mock, store } = render('maintenance');
      await settle(f);
      const day = (w: number, name: string) => [...el.querySelectorAll('li.win')[w]!.querySelectorAll<HTMLButtonElement>('mat-button-toggle button')].find((b) => b.textContent?.trim() === name)!;
      day(0, 'Sat').click();
      await settle(f);
      expect(day(0, 'Sat').getAttribute('aria-pressed')).toBe('true');
      expect(btn(el, 'Save windows').disabled).toBe(false);
      const start = el.querySelector<HTMLInputElement>('input[aria-label="From, window 1"]')!;
      start.value = '21:30';
      start.dispatchEvent(new Event('input'));
      start.value = 'bad';
      start.dispatchEvent(new Event('input'));
      await settle(f);
      expect(el.textContent).toContain('21:30 to 02:00');
      const end = el.querySelector<HTMLInputElement>('input[aria-label="To, window 1"]')!;
      end.value = '21:30';
      end.dispatchEvent(new Event('input'));
      await settle(f);
      expect(el.querySelector('[role="alert"]')!.textContent).toContain('different time');
      expect(btn(el, 'Save windows').disabled).toBe(true);
      end.value = '03:00';
      end.dispatchEvent(new Event('input'));
      btn(el, 'Add window').click();
      await settle(f);
      expect(el.querySelectorAll('li.win')).toHaveLength(3);
      (el.querySelector('[aria-label="Remove window 3"]') as HTMLElement).click();
      await settle(f);
      const spy = vi.spyOn(mock, 'saveSettings');
      btn(el, 'Save windows').click();
      await settle(f);
      expect(spy.mock.calls[0]![0].maintenance.windows[0]).toMatchObject({ startMinute: 21 * 60 + 30, endMinute: 180 });
      expect(store.maintenanceError()).toBeNull();
      expect(btn(el, 'Save windows').disabled).toBe(true);
    });

    it('turning it on with no windows adds one; off dims the windows; discard goes back; the engine refusal is shown', async () => {
      const { f, el, mock, store } = render('maintenance');
      const real = await mock.settings();
      real.maintenance = { enabled: false, windows: [] } as MaintenanceSettings;
      await mock.saveSettings(real);
      await TestBed.inject(SettingsStore).load();
      await settle(f);
      expect(el.querySelectorAll('li.win')).toHaveLength(0);
      (el.querySelector('[aria-label="Only run heavier work in maintenance windows"]') as HTMLElement).click();
      await settle(f);
      expect(el.querySelectorAll('li.win')).toHaveLength(1);
      btn(el, 'Discard changes').click();
      await settle(f);
      expect(el.querySelectorAll('li.win')).toHaveLength(0);
      (el.querySelector('[aria-label="Only run heavier work in maintenance windows"]') as HTMLElement).click();
      await settle(f);
      vi.spyOn(mock, 'saveSettings').mockRejectedValueOnce({ code: 'badRequest', message: 'The engine says no.' });
      btn(el, 'Save windows').click();
      await settle(f);
      expect(el.querySelector('[role="alert"]')!.textContent).toContain('The engine says no.');
      expect(store.maintenanceSaving()).toBe(false);
      for (let i = 0; i < 14; i++) btn(el, 'Add window').click();
      await settle(f);
      expect(el.textContent).toContain('At most 14 windows.');
      (el.querySelector('[aria-label="Only run heavier work in maintenance windows"]') as HTMLElement).click();
      await settle(f);
      expect(el.querySelector('li.win.off')).not.toBeNull();
    });

    it('shows an error with Retry when the settings cannot be read', async () => {
      const { f, el } = render('maintenance', { fail: new Set(['settings']) });
      await settle(f);
      expect(el.querySelector('[role="alert"]')).not.toBeNull();
      expect(btn(el, 'Retry')).toBeTruthy();
    });
  });
});
