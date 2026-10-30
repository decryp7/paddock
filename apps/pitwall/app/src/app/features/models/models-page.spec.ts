import { pickOption, dialogsAnswering } from '../../../testing/render';
import { TestbedHarnessEnvironment } from '@angular/cdk/testing/testbed';
import { TestBed } from '@angular/core/testing';
import { MatTableHarness } from '@angular/material/table/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { Download, Fit } from '../../core/api.types';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { fitDetail } from './discover-tab';
import { etaMs } from './downloads-list';
import { contextText, idleOptions, paramsText } from './library-tab';
import { ModelsPage } from './models-page';
import { ModelsStore } from './models.store';


const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

async function render(backend: PitwallBackend = new MockBackend(), confirm = true) {
  TestBed.configureTestingModule({
    imports: [ModelsPage],
    providers: [
      { provide: BACKEND, useValue: backend },
      dialogsAnswering(confirm),
    ],
  });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
  const fixture = TestBed.createComponent(ModelsPage);
  fixture.detectChanges();
  await tick();
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    await tick();
    fixture.detectChanges();
    await tick();
    fixture.detectChanges();
  };
  const button = (text: string) => Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.trim().startsWith(text)) as HTMLButtonElement;
  /** Opens a model's details by clicking its row. */
  const open = async (name: string) => {
    const row = Array.from(el.querySelectorAll<HTMLElement>('pw-library-tab tbody tr')).find((r) => r.textContent!.includes(name))!;
    row.click();
    await settle();
    return el.querySelector('pw-detail-panel') as HTMLElement;
  };
  /** Opens a row's "..." menu and runs one item by its label. */
  const menu = async (name: string, item: string) => {
    (el.querySelector(`button[aria-label="More actions for ${name}"]`) as HTMLButtonElement).click();
    await settle();
    const entry = Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-item')).find((b) => b.textContent?.trim() === item)!;
    entry.click();
    await settle();
  };
  return { fixture, el, settle, button, live, open, menu };
}

describe('models helpers', () => {
  it('formats params, context and idle choices', () => {
    expect(paramsText(null)).toBe('—');
    expect(paramsText(8.2)).toBe('8.2B');
    expect(paramsText(12.2)).toBe('12B');
    expect(paramsText(0.14)).toBe('140M');
    expect(contextText(null)).toBe('—');
    expect(contextText(32768)).toBe('32k');
    expect(contextText(512)).toBe('512');
    expect(idleOptions(null)).toEqual([60, 120, 300, 600, 1800, 3600]);
    expect(idleOptions(900)).toContain(900);
    expect(idleOptions(300).length).toBe(6);
  });

  it('describes a fit and a time left', () => {
    const fit: Fit = { verdict: 'tight', needRam: 2 * 1024 ** 3, needVram: 3 * 1024 ** 3, haveRam: 0, haveVram: 0, offload: 'partial', message: '' };
    expect(fitDetail(fit)).toBe('3.0 GB VRAM + 2.0 GB RAM · part of the layers on the GPU');
    expect(fitDetail({ ...fit, needRam: 0, needVram: 0, offload: 'none' })).toBe('Nothing · CPU only');
    expect(fitDetail({ ...fit, needRam: 0, offload: 'all' })).toContain('all layers on the GPU');
    const d = { state: 'downloading', speedBps: 10, bytesDone: 0, bytesTotal: 100 } as Download;
    expect(etaMs(d)).toBe(10_000);
    expect(etaMs({ ...d, speedBps: 0 })).toBeNull();
    expect(etaMs({ ...d, state: 'paused' })).toBeNull();
  });
});

describe('ModelsPage', () => {
  it('has a Presets tab that lists the presets', async () => {
    const { el, settle, fixture } = await render();
    expect(Array.from(el.querySelectorAll('[role="tab"]')).map((t) => t.textContent!.trim().replace(/\s+\d+$/, ''))).toEqual(['Library', 'Discover', 'Presets', 'Downloads']);
    fixture.componentInstance.tab.set('presets');
    await settle();
    expect(el.querySelector('pw-presets-tab')?.textContent).toContain('debrief-explain');
  });

  it('lists the library with disk usage, status and actions', async () => {
    const { el } = await render();
    const rows = el.querySelectorAll('pw-library-tab tbody tr');
    expect(rows.length).toBe(8);
    expect(rows[0]!.textContent).toContain('Qwen3.5 9B');
    expect(rows[0]!.textContent).toContain('Loaded');
    expect(el.textContent).toContain('Partial downloads');
    expect(el.textContent).toContain('1 unused for 30+ days');
    expect(el.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Library');
    // Nothing destructive at rest: Delete lives in the "..." menu only.
    expect(el.querySelector('button[aria-label="Delete Gemma 4 12B"]')).toBeNull();
    expect(el.querySelector('button[aria-label="More actions for Gemma 4 12B"]')).not.toBeNull();
    // Units in the header, so the cells are bare right-aligned numbers.
    expect(Array.from(el.querySelectorAll('pw-library-tab thead th')).map((t) => t.textContent!.trim())).toEqual(['Model', 'Size GB', 'Params', 'Context tokens', 'Status', 'Actions']);
    expect(rows[0]!.querySelector('td.mat-column-size')!.textContent!.trim()).toMatch(/^\d+(\.\d)?$/);
    expect(rows[0]!.querySelector('mat-icon[aria-label="Chat"]')).not.toBeNull();
    expect(rows[0]!.querySelector('.tag, .pill')).toBeNull();
  });

  it('names each capability glyph for a pointer and a screen reader', async () => {
    const { el } = await render();
    const glyphs = Array.from(el.querySelectorAll('pw-library-tab tbody .glyphs mat-icon'));
    expect(glyphs.length).toBeGreaterThan(8);
    for (const g of glyphs) {
      expect(g.getAttribute('role')).toBe('img');
      expect(g.getAttribute('aria-label')).toMatch(/\w/);
      expect(g.classList).toContain('mat-mdc-tooltip-trigger');
    }
  });

  it('says a value every model shares once in the toolbar instead of in a constant column', async () => {
    const backend = new MockBackend();
    const all = await backend.localModels();
    backend.localModels = async () => all.map((m) => ({ ...m, contextMax: 32_768 }));
    const { el } = await render(backend);
    expect(Array.from(el.querySelectorAll('pw-library-tab thead th')).map((t) => t.textContent!.trim())).not.toContain('Context tokens');
    expect(el.querySelector('pw-library-tab .toolbar')!.textContent).toContain('all 32k context');
    // A column that varies is shown, so its value is not repeated in the toolbar.
    expect(el.querySelector('pw-library-tab .toolbar')!.textContent).not.toMatch(/all \d[\d.]*[BM]\b/);
  });

  it('is a Material table with one sticky header row (rule 47, ADR 0027)', async () => {
    const { fixture } = await render();
    const loader = TestbedHarnessEnvironment.loader(fixture);
    const table = await loader.getHarness(MatTableHarness.with({ selector: 'pw-library-tab table' }));
    const [header] = await table.getHeaderRows();
    expect(await header!.getCellTextByIndex()).toEqual(['Model', 'Size GB', 'Params', 'Context tokens', 'Status', 'Actions']);
    const rows = await table.getRows();
    expect(rows.length).toBe(8);
    const first = await rows[0]!.getCellTextByColumnName();
    expect(first['model']).toContain('Qwen3.5 9B');
    expect(first['status']).toContain('Loaded');
    expect(fixture.nativeElement.querySelector('pw-library-tab thead th')!.classList).toContain('mat-mdc-table-sticky');
  });

  it('opens the details on a row click or Enter and closes them with Escape, returning focus to the row', async () => {
    const { el, settle, open } = await render();
    expect(el.querySelector('pw-detail-panel')).toBeNull();
    const panel = await open('Gemma 4 12B');
    expect(panel.textContent).toContain('Gemma 4 12B');
    expect(panel.textContent).toContain('unsloth/gemma-4-12b-it-GGUF');
    expect(panel.textContent).toContain('Q4_K_M');
    expect(el.querySelector('tbody tr.selected')?.textContent).toContain('Gemma 4 12B');
    const row = el.querySelector<HTMLElement>('#row-gemma-4-12b-it-q4_k_m')!;
    row.focus();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle();
    expect(el.querySelector('pw-detail-panel')).toBeNull();
    expect(document.activeElement).toBe(row);
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await settle();
    expect(el.querySelector('pw-detail-panel')).not.toBeNull();
    el.querySelector<HTMLButtonElement>('pw-detail-panel button[aria-label="Close details"]')!.click();
    await settle();
    expect(el.querySelector('pw-detail-panel')).toBeNull();
  });

  it('moves between rows with the arrow keys and opens the menu with Shift+F10 and right-click', async () => {
    const { el, settle } = await render();
    const rows = Array.from(el.querySelectorAll<HTMLElement>('pw-library-tab tbody tr'));
    rows[0]!.focus();
    rows[0]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(rows[1]);
    rows[1]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    await settle();
    const labels = () => Array.from(document.querySelectorAll('.mat-mdc-menu-item')).map((b) => b.textContent!.trim());
    expect(labels()).toContain('Delete…');
    expect(document.querySelector('.mat-mdc-menu-item.danger')?.textContent).toContain('Delete');
    const close = async () => {
      document.querySelector<HTMLElement>('.mat-mdc-menu-panel')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true }));
      await settle();
    };
    await close();
    rows[2]!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 50, clientY: 50 }));
    await settle();
    expect(labels().filter((l) => l === 'Unload' || l === 'Load').length).toBeGreaterThan(0);
    await close();
  });

  it('loads, pins, sets idle, deletes and cleans up from the table and its details', async () => {
    const backend = new MockBackend();
    const { el, settle, button, open, menu } = await render(backend);
    (el.querySelector('button[aria-label="Load Gemma 4 12B"]') as HTMLButtonElement).click();
    await settle();
    expect(el.querySelector('button[aria-label="Unload Gemma 4 12B"]')).not.toBeNull();
    const panel = await open('Gemma 4 12B');
    Array.from(panel.querySelectorAll('button')).find((b) => b.textContent!.includes('Pin (keep loaded)'))!.click();
    await settle();
    expect(el.querySelector('pw-detail-panel')!.textContent).toContain('Unpin');
    expect(el.querySelector('#row-gemma-4-12b-it-q4_k_m mat-icon[aria-label="Pinned"]')).not.toBeNull();

    const id = 'qwen3.5-4b-q4_k_m';
    const idleOf = async () => (await backend.localModels()).find((m) => m.id === id)?.idleSecs;
    await open('Qwen3.5 4B');
    const choose = (label: string) => pickOption(el.querySelector(`#idle-qwen3\\.5-4b-q4_k_m`) as HTMLElement, label, settle);
    await choose('After 10 min');
    expect(await idleOf()).toBe(600);
    expect((el.querySelector(`#idle-qwen3\\.5-4b-q4_k_m`) as HTMLElement).textContent).toContain('After 10 min');
    await choose('Global setting');
    expect(await idleOf()).toBeNull();

    await menu('BGE reranker v2 m3', 'Delete…');
    expect(el.textContent).not.toContain('BGE reranker v2 m3');
    button('Clean up unused').click();
    await settle();
    expect(el.textContent).not.toContain('Qwen3.5 2B');
  });

  it('does nothing when a confirmation is declined', async () => {
    const { el, settle, button, menu } = await render(new MockBackend(), false);
    await menu('BGE reranker v2 m3', 'Delete…');
    button('Clean up unused').click();
    await settle();
    expect(el.textContent).toContain('BGE reranker v2 m3');
    expect(el.textContent).toContain('Qwen3.5 2B');
  });

  it('shows the empty library with a way to discover models', async () => {
    const { el, settle, button } = await render(new MockBackend({ empty: true }));
    expect(el.textContent).toContain('No models yet');
    button('Discover models').click();
    await settle();
    expect(el.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Discover');
  });

  it('shows an error with Retry when the library fails', async () => {
    const { el } = await render(new MockBackend({ failing: ['localModels'] }));
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('not responding');
  });

  it('searches, opens a repo with fit verdicts and downloads a variant with its projector', async () => {
    const backend = new MockBackend();
    const { el, settle, fixture } = await render(backend);
    (el.querySelector('#tab-discover') as HTMLElement).click();
    await settle();
    expect(el.querySelectorAll('pw-discover-tab .hit').length).toBe(12);
    const input = el.querySelector('#hf-q') as HTMLInputElement;
    input.value = 'gemma';
    (el.querySelector('form[role="search"]') as HTMLFormElement).dispatchEvent(new Event('submit'));
    await settle();
    const hits = el.querySelectorAll<HTMLButtonElement>('pw-discover-tab .hit');
    expect(hits.length).toBe(3);
    hits[0]!.click();
    await settle();
    expect(el.textContent).toContain('vision projector');
    const rows = el.querySelectorAll('pw-discover-tab tbody tr');
    expect(rows.length).toBe(8);
    expect(el.textContent).toContain('Tight');
    expect(el.textContent).toContain("Won't fit");
    expect(el.textContent).toContain('In library');
    expect(el.querySelector('pw-discover-tab tbody')?.textContent).toMatch(/up to \d+ requests at once|one request at a time/);
    (el.querySelector('pw-discover-tab mat-slide-toggle button') as HTMLButtonElement).click();
    fixture.detectChanges();
    (el.querySelector('button[aria-label^="Download gemma-4-12b-it-Q2_K"]') as HTMLButtonElement).click();
    await settle();
    const d = (await backend.downloads())[0]!;
    expect(d.name).toBe('gemma-4-12b-it-Q2_K.gguf');
    (el.querySelector('button[aria-label="Close repo"]') as HTMLButtonElement).click();
    await settle();
    expect(el.textContent).toContain('Pick a repo');
  });

  it('lists recommendations as one-line rows with a verdict dot and a hover download, and no repeated tags', async () => {
    const { el, settle, button } = await render();
    (el.querySelector('#tab-discover') as HTMLButtonElement).click();
    await settle();
    const rows = el.querySelectorAll('pw-discover-tab .rec-list .rec');
    expect(rows.length).toBe(7);
    expect(rows[0]!.querySelector('.status')).not.toBeNull();
    expect(rows[0]!.querySelector('.pill, .tag')).toBeNull();
    expect(Array.from(rows).find((r) => r.querySelector('button.reveal'))?.querySelector('button.reveal')?.getAttribute('aria-label')).toMatch(/^Download /);
    // Every result carries "gguf": it is not information, so no tag chips at all.
    expect(el.querySelector('pw-discover-tab .hit .tag')).toBeNull();
    // Search is the page's primary action only while the field has text.
    const search = Array.from(el.querySelectorAll('pw-discover-tab form button')).find((b) => b.textContent!.trim() === 'Search') as HTMLButtonElement;
    expect(search.classList.contains('mat-mdc-unelevated-button')).toBe(false);
    const input = el.querySelector('#hf-q') as HTMLInputElement;
    input.value = 'qwen';
    input.dispatchEvent(new Event('input'));
    await settle();
    expect(search.classList.contains('mat-mdc-unelevated-button')).toBe(true);
    expect(button).toBeDefined();
  });

  it('shows downloads with progress, speed and the resumable state, and cancels them', async () => {
    const backend = new MockBackend();
    const { el, settle, live } = await render(backend);
    (el.querySelector('#tab-downloads') as HTMLButtonElement).click();
    await settle();
    expect(el.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Downloads');
    const items = el.querySelectorAll('pw-downloads-list li');
    expect(items.length).toBe(2);
    // A running download shows its bar and speed, not a status word; only the exceptions get one.
    expect(items[0]!.querySelector('.status')).toBeNull();
    expect(items[0]!.querySelector('[role="progressbar"]')).not.toBeNull();
    expect(items[0]!.textContent).toContain('/s');
    expect(items[1]!.textContent).toContain('Paused · resumable');
    expect(items[1]!.textContent).toContain('Connection reset');
    live.connect();
    (el.querySelector('button[aria-label="Cancel gemma-4-E4B-it-Q8_0.gguf"]') as HTMLButtonElement).click();
    await settle();
    expect((await backend.downloads())[0]!.state).toBe('cancelled');
    expect(live.downloads()[0]!.state).toBe('cancelled');
    live.disconnect();
  });

  it('refreshes the library when a model download finishes', async () => {
    const backend = new MockBackend();
    const spy = vi.spyOn(backend, 'localModels');
    const { settle, live } = await render(backend);
    const before = spy.mock.calls.length;
    live.connect();
    for (let i = 0; i < 40; i++) backend.tick();
    await settle();
    expect(live.downloads().some((d) => d.state === 'done')).toBe(true);
    expect(spy.mock.calls.length).toBeGreaterThan(before);
    live.disconnect();
  });

  it('shows an empty downloads list', async () => {
    const { el, settle } = await render(new MockBackend({ empty: true }));
    (el.querySelector('#tab-downloads') as HTMLButtonElement).click();
    await settle();
    expect(el.textContent).toContain('No downloads');
  });

  it('has a Parallel slots setting in the details that opens the slots panel, saves a count and says a loaded model keeps its own', async () => {
    const backend = new MockBackend();
    const { el, settle, fixture, open } = await render(backend);
    expect(el.querySelector('pw-library-tab thead')?.textContent).not.toContain('Slots');
    await open('Qwen3.5 4B');
    expect(el.querySelector('button[aria-label^="Parallel slots for Qwen3.5 4B"]')?.textContent).toContain('4 slots');
    await open('Gemma 4 12B');
    const trigger = el.querySelector('button[aria-label="Parallel slots for Gemma 4 12B: 2 slots (default)"]') as HTMLButtonElement;
    expect(trigger).not.toBeNull();
    trigger.click();
    await settle();
    const panel = document.querySelector('pw-slots-panel') as HTMLElement;
    expect(panel.textContent).toContain('Parallel slots · Gemma 4 12B');
    (Array.from(panel.querySelectorAll('[role="radio"]')).find((b) => b.querySelector('b')?.textContent === '4') as HTMLButtonElement).click();
    fixture.detectChanges();
    (Array.from(panel.querySelectorAll('button')).find((b) => b.textContent?.startsWith('Save')) as HTMLButtonElement).click();
    await settle();
    expect(document.querySelector('pw-slots-panel')).toBeNull();
    expect((await backend.localModels()).find((m) => m.id === 'gemma-4-12b-it-q4_k_m')?.slots).toBe(4);
    expect(el.querySelector('button[aria-label="Parallel slots for Gemma 4 12B: 4 slots"]')).not.toBeNull();
    // A loaded model with a different saved count explains it on hover.
    await backend.setModelSlots('qwen3.5-9b-q4_k_m', 8);
    const store = fixture.debugElement.injector.get(ModelsStore);
    await store.load();
    fixture.detectChanges();
    await open('Qwen3.5 9B');
    el.querySelector('button[aria-label^="Parallel slots for Qwen3.5 9B"]')!.dispatchEvent(new Event('mouseenter'));
    await settle();
    expect(document.body.textContent).toContain('Loaded with 2 slots; 8 slots apply the next time it loads');
  });

  it('shows a banner, a glyph on the row and the update in the details, and Get it asks first', async () => {
    const backend = new MockBackend({ modelUpdates: true });
    const spy = vi.spyOn(backend, 'downloadModel');
    const { el, settle, open } = await render(backend, false);
    expect(el.querySelector('pw-update-banner .banner')!.textContent).toContain('2 updates available');
    const marks = Array.from(el.querySelectorAll('tbody mat-icon.upd')).map((n) => n.getAttribute('aria-label'));
    expect(marks).toEqual(['Update available for Qwen3.5 9B', 'Update available for Qwen3.5 4B']);
    const row = (name: string) => Array.from(el.querySelectorAll('tbody tr')).find((r) => r.textContent!.includes(name))!;
    expect(row('Gemma 4 12B').querySelector('.upd')).toBeNull();
    const panel = await open('Qwen3.5 9B');
    expect(panel.querySelector('.callout')!.textContent).toContain('Update available: newer revision');
    (panel.querySelector('.callout button') as HTMLButtonElement).click();
    await settle();
    expect(spy).not.toHaveBeenCalled();
  });

  it('shows no update notes when nothing is on offer', async () => {
    const { el } = await render();
    expect(el.querySelector('.upd')).toBeNull();
    expect(el.querySelector('pw-update-banner .banner')).toBeNull();
  });
});
