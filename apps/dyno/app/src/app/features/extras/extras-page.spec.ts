import { Component, input } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, provideRouter, Router } from '@angular/router';
import type { JobOutput, MaintenanceSettings, SleepReport } from '../../core/api.types';
import { mockBoot, mockSoftware, mockUsage } from '../../core/mock/extras';
import { BACKEND } from '../../core/backend';
import type { MockBackend } from '../../core/mock-backend';
import { jobsProviders, mockWithJobs } from '../../../testing/job-test-support';
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
  const { mock, jobs } = mockWithJobs({ now: () => NOW, ...options });
  TestBed.configureTestingModule({
    providers: [
      provideIcons(),
      jobsProviders(jobs),
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
    // A job the mock runs steps on a timer, like the agent's pushes.
    await new Promise((r) => setTimeout(r, 2));
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
    f.detectChanges();
  }
};
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text))!;
const tabBtn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((b) => b.textContent === text)!;
const rowsOf = (el: HTMLElement, label: string) => el.querySelectorAll(`[role$="grid"][aria-label="${label}"] [role="row"][data-key]`);
const groupsOf = (el: HTMLElement, label: string) => [...el.querySelectorAll(`[role$="grid"][aria-label="${label}"] .pd-group`)];
const clickTab = async (f: Parameters<typeof settle>[0], el: HTMLElement, text: string) => {
  tabBtn(el, text).click();
  await settle(f);
};

/** The next job's result is `output`: the mock stands in for the engine's report. */
const returns = (mock: MockBackend, output: JobOutput) => vi.spyOn(mock, 'jobResult').mockResolvedValue(output);

describe('dy-extras-page', () => {
  it('shows the software list an earlier run saved at once, with its age, and Refresh has the fresh one', async () => {
    vi.useFakeTimers({ now: NOW, toFake: ['Date'] });
    const { f, el, store } = render('inventory', { savedListsAgeMs: 3 * 3_600_000 });
    await settle(f);
    expect(store.software().data!.savedAt).toBe(NOW - 3 * 3_600_000);
    const note = el.querySelector('[data-testid="saved-list"]')!.textContent!.replace(/\s+/g, ' ');
    expect(note).toContain('Last known list, read 3 h ago.');
    expect(el.textContent).toContain('Visual Studio Code');
    btn(el, 'Refresh').click();
    await settle(f);
    expect(store.software().data!.savedAt).toBeNull();
    expect(el.querySelector('[data-testid="saved-list"]')).toBeNull();
    vi.useRealTimers();
  });

  it('has the eight tabs in a tablist, opens What changed first, and reads each tab only when opened', async () => {
    const { f, el, mock } = render();
    const spy = vi.spyOn(mock, 'startJob');
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
    const table = el.querySelector('[role$="grid"][aria-label="What changed"]')!;
    expect(table.querySelectorAll('[role="columnheader"]').length).toBeGreaterThan(3);
    expect(groupsOf(el, 'What changed').length).toBeGreaterThanOrEqual(3);
    expect(groupsOf(el, 'What changed')[0]!.textContent).toContain('Today');
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

  it('What changed: Collapse all folds every day to its heading and Expand all opens them', async () => {
    const { f, el } = render();
    await settle(f);
    const days = groupsOf(el, 'What changed').length;
    const lines = rowsOf(el, 'What changed').length;
    el.querySelector<HTMLButtonElement>('[data-testid=tree-collapse-all]')!.click();
    await settle(f);
    expect(groupsOf(el, 'What changed')).toHaveLength(days);
    expect(rowsOf(el, 'What changed')).toHaveLength(0);
    el.querySelector<HTMLButtonElement>('[data-testid=tree-expand-all]')!.click();
    await settle(f);
    expect(rowsOf(el, 'What changed')).toHaveLength(lines);
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
    const report: SleepReport = { blockers: [], wakes: [], blockersUnavailable: 'pmset is not available here.', wakesUnavailable: null, blockersVia: 'local' };
    returns(mock, { sleep: report });
    await TestBed.inject(ExtrasStore).reload('sleep');
    await settle(f);
    expect(el.querySelector('.none')!.textContent).toContain('pmset is not available here.');
    expect(el.textContent).toContain('No wakes recorded');
    returns(mock, { sleep: { ...report, blockersUnavailable: null, wakesUnavailable: 'No log.' } });
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
    const real = mockBoot(NOW);
    returns(mock, { boot: { ...real, slowest: [], stepsUnavailable: 'Windows has no boot trace here.' } });
    await store.reload('boot');
    await settle(f);
    expect(el.querySelector('p.note')!.textContent).toContain('Windows has no boot trace here.');
    returns(mock, { boot: { ...real, slowest: [], stepsUnavailable: null, boots: real.boots.slice(0, 1) } });
    await store.reload('boot');
    await settle(f);
    expect(el.textContent).toContain('Nothing slowed the last start-up');
    expect(el.querySelector('dy-chart')).toBeNull();
    returns(mock, { boot: { boots: [], slowest: [], lastTotalSecs: null, verdict: null, stepsUnavailable: 'No history.' } });
    await store.reload('boot');
    await settle(f);
    expect(el.querySelector('.none')!.textContent).toContain('No history.');
    returns(mock, { boot: { ...real, stepsUnavailable: 'Only the top ones.' } });
    await store.reload('boot');
    await settle(f);
    expect(el.textContent).toContain('Only the top ones.');
  });

  it('Apps: sortable, units from the formatters, notes when a part is unavailable', async () => {
    const { f, el, mock, store } = render('apps');
    await settle(f);
    const names = () => [...rowsOf(el, 'Network and energy per app')].map((r) => r.querySelector('.nm')!.textContent);
    const header = (name: string) => [...el.querySelectorAll<HTMLElement>('[role="columnheader"]')].find((h) => h.textContent!.trim() === name)!;
    const sortBy = (name: string) => (header(name).querySelector('.mat-sort-header-container') as HTMLElement).click();
    expect(names()[0]).toBe('Google Chrome');
    expect(el.textContent).toContain('4.8 GB');
    sortBy('Sent');
    await settle(f);
    expect(header('Sent').getAttribute('aria-sort')).toBe('ascending');
    sortBy('Sent');
    await settle(f);
    expect(names()[0]).toBe('zoom.us');
    expect(header('Sent').getAttribute('aria-sort')).toBe('descending');
    sortBy('App');
    await settle(f);
    expect(header('App').getAttribute('aria-sort')).toBe('ascending');
    expect(names()[0]).toBe('Google Chrome');
    const real = mockUsage();
    returns(mock, { appUsage: { ...real, networkUnavailable: 'Needs nettop.', energyUnavailable: 'No energy data.' } });
    await store.reload('apps');
    await settle(f);
    expect(el.textContent).toContain('Needs nettop.');
    expect(el.textContent).toContain('No energy data.');
    expect(header('Received')).toBeUndefined();
    returns(mock, { appUsage: { apps: [], networkUnavailable: null, energyUnavailable: null } });
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
    expect(rowsOf(el, 'Installed software').length).toBeGreaterThanOrEqual(40);
    const search = el.querySelector<HTMLInputElement>('input[type="search"]')!;
    search.value = 'apple';
    search.dispatchEvent(new Event('input'));
    await settle(f);
    expect(rowsOf(el, 'Installed software').length).toBeGreaterThan(5);
    expect(el.textContent).toMatch(/\d+ of 40 programs/);
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
    const real = mockSoftware();
    returns(mock, { software: { ...real, items: real.items.slice(0, 2), truncated: true } });
    await store.reload('inventory');
    await settle(f);
    expect(el.textContent).toContain('more packages than Dyno lists');
    returns(mock, { software: { items: [], truncated: false, unavailable: 'No package manager.', savedAt: null } });
    await store.reload('inventory');
    await settle(f);
    expect(el.querySelector('.none')!.textContent).toContain('No package manager.');
    returns(mock, { software: { items: [], truncated: false, unavailable: null, savedAt: null } });
    await store.reload('inventory');
    await settle(f);
    expect(el.querySelector('.none strong')!.textContent).toBe('No software found');
    vi.spyOn(mock, 'saveInventoryReport').mockRejectedValueOnce({ code: 'accessDenied', message: 'Read-only folder.' });
    await store.exportReport('json');
    await settle(f);
    expect(el.querySelector('.note.bad')!.textContent).toContain('Read-only folder.');
  });

  it('every tab shows an inline error with Retry', async () => {
    const fails: [string, string][] = [['What changed', 'changes'], ['Sleep', 'startJob'], ['Boot', 'startJob'], ['Apps', 'startJob'], ['GPU', 'gpuProcesses'], ['Inventory', 'startJob']];
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
    const table = el.querySelector('[role$="grid"][aria-label="Large and duplicate files"]')!;
    expect(table.querySelectorAll('[role="columnheader"]')).toHaveLength(3);
    expect(groupsOf(el, 'Large and duplicate files').map((g) => g.querySelector('.pd-group-name')!.textContent)).toEqual(['Large files', '3 copies of site-redesign.psd', '2 copies of resnet50.onnx']);
    expect(table.querySelectorAll('input[type="checkbox"]')).toHaveLength(0);
    expect(rowsOf(el, 'Large and duplicate files')).toHaveLength(8 + 5);
    expect(el.textContent).toContain('Copies are listed oldest first.');
    expect(btn(el, 'Choose another folder')).toBeTruthy();
  });

  it('Files: filters, collapse and the empty result work', async () => {
    const { f, el, mock, store } = render('files');
    await settle(f);
    btn(el, 'Choose a folder').click();
    await settle(f);
    (groupsOf(el, 'Large and duplicate files')[0] as HTMLElement).click();
    await settle(f);
    expect(rowsOf(el, 'Large and duplicate files').length).toBe(5);
    btn(el, 'Duplicates').click();
    await settle(f);
    expect(store.mode()).toBe('duplicates');
    btn(el, 'Large').click();
    await settle(f);
    btn(el, '1 GB').click();
    await settle(f);
    expect(store.minMib()).toBe(1024);
    expect(el.querySelector('.none strong')).toBeNull();
    const limit = { limit: 'files' as const, message: 'Stopped after 500000 files and folders. Choose a smaller folder to see everything in it.' };
    vi.spyOn(mock, 'jobResult').mockResolvedValue({ finder: { scanId: 's', folder: '/x', large: [], duplicates: [], scanned: 10, unreadable: 0, limit, wastedBytes: 0 } });
    await store.search();
    await settle(f);
    expect(el.querySelector('.none strong')!.textContent).toBe('Nothing found in this folder');
    expect(el.textContent).toContain('before a limit. Stopped after 500000 files and folders. Choose a smaller folder');
    vi.spyOn(mock, 'startJob').mockRejectedValue({ code: 'badRequest', message: 'Choose a folder with a full path.' });
    await store.search();
    await settle(f);
    expect(el.querySelector('[role="alert"]')!.textContent).toContain('Choose a folder with a full path.');
  });

  it('Files: Collapse all and Expand all fold every group', async () => {
    const { f, el } = render('files');
    await settle(f);
    btn(el, 'Choose a folder').click();
    await settle(f);
    const groups = groupsOf(el, 'Large and duplicate files').length;
    el.querySelector<HTMLButtonElement>('[data-testid=tree-collapse-all]')!.click();
    await settle(f);
    expect(groupsOf(el, 'Large and duplicate files')).toHaveLength(groups);
    expect(rowsOf(el, 'Large and duplicate files')).toHaveLength(0);
    el.querySelector<HTMLButtonElement>('[data-testid=tree-expand-all]')!.click();
    await settle(f);
    expect(rowsOf(el, 'Large and duplicate files')).toHaveLength(13);
  });

  it('Files: shows the search as a job with its count and Cancel, then the stopped note with Search again', async () => {
    const { f, el, mock, store } = render('files');
    await settle(f);
    mock.jobSteps = 40;
    mock.jobStepMs = 20;
    btn(el, 'Choose a folder').click();
    for (let i = 0; i < 5 && !el.querySelector('pd-job'); i++) {
      await new Promise((r) => setTimeout(r, 20));
      await settle(f);
    }
    const card = el.querySelector('pd-job')!;
    expect(card.textContent).toContain('Finding files');
    expect(card.textContent).toContain('/Users/alex/Projects');
    expect(card.textContent).toMatch(/Listing files/);
    expect(el.querySelector('.skeleton-wrap')).toBeNull();
    expect(btn(el, 'Choose another folder')?.disabled ?? true).toBe(true);
    btn(card as HTMLElement, 'Cancel').click();
    for (let i = 0; i < 40 && store.finder().loading; i++) {
      await new Promise((r) => setTimeout(r, 25));
      await settle(f);
    }
    expect(store.finderCancelled()).toBe(true);
    expect(el.querySelector('pd-job')).toBeNull();
    expect(el.textContent).toContain('Search stopped.');
    expect(el.querySelector('[role="alert"]')).toBeNull();
    mock.jobSteps = 1;
    btn(el, 'Search again').click();
    await settle(f);
    await new Promise((r) => setTimeout(r, 20));
    await settle(f);
    expect(el.querySelector('[role$="grid"][aria-label="Large and duplicate files"]')).not.toBeNull();
    expect(el.textContent).not.toContain('Search stopped.');
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
