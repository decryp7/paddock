import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { provideRouter } from '@angular/router';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import type { Overview } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { ThemeService } from '../../core/theme';
import { AppearanceStore, BrowserAppearanceBackend, provideAppearance } from '@decryp7/paddock-ui/shell';
import { registerIcons } from '../../shared/icons';
import { FakeChart, fakeChartProvider, settle } from '../../../testing/render';
import { BROWSER_VERSION } from '../../shell/engines';
import { Dashboard, PerfMeter, shortModel } from './dashboard';

async function mount(opts: { backend?: MockBackend; load?: boolean; patch?: (o: Overview) => Overview } = {}) {
  const backend = opts.backend ?? new MockBackend();
  FakeChart.all = [];
  TestBed.configureTestingModule({ imports: [Dashboard], providers: [{ provide: BACKEND, useValue: backend }, provideRouter([]), fakeChartProvider, provideAppearance({ accent: 'pitwall', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) })] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  if (opts.load !== false) await live.load();
  if (opts.patch) patchState(live as unknown as WritableStateSource<object>, { overview: opts.patch(live.overview()!) });
  const fixture = TestBed.createComponent(Dashboard);
  await settle(fixture);
  const el = fixture.nativeElement as HTMLElement;
  const btn = (label: string) => Array.from(el.querySelectorAll('button')).find((b) => b.textContent!.trim() === label || b.textContent!.includes(label)) as HTMLButtonElement;
  /** Opens the header's More actions menu and runs one item. */
  const more = async (item: string) => {
    (el.querySelector('button[aria-label="More actions"]') as HTMLButtonElement).click();
    await settle(fixture);
    Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-item')).find((b) => b.textContent!.trim() === item)!.click();
    await settle(fixture);
  };
  return { fixture, el, live, backend, btn, more };
}

describe('shortModel', () => {
  it('drops a trailing quantisation suffix', () => {
    expect(shortModel('gemma-4-12b-it-q4_k_m')).toBe('gemma-4-12b-it');
    expect(shortModel('qwen3.5-4b-Q4_K_M')).toBe('qwen3.5-4b');
    expect(shortModel('qwen3-embedding-0.6b-q8_0')).toBe('qwen3-embedding-0.6b');
    expect(shortModel('model.bf16')).toBe('model');
    expect(shortModel('model-f32')).toBe('model');
  });
  it('leaves ids without one alone', () => {
    expect(shortModel('plain-model')).toBe('plain-model');
    expect(shortModel('')).toBe('');
  });
});

describe('PerfMeter', () => {
  it('returns the measured value and counts builds', () => {
    const m = new PerfMeter();
    expect(m.avg).toBe(0);
    expect(m.measure(() => 42)).toBe(42);
    expect(m.count).toBe(1);
    expect(m.last).toBeGreaterThanOrEqual(0);
    expect(m.avg).toBe(m.last);
  });
  it('averages over a rolling window of the last 60 samples', () => {
    const m = new PerfMeter();
    const now = vi.spyOn(performance, 'now');
    for (let i = 0; i < 70; i++) {
      now.mockReturnValueOnce(0).mockReturnValueOnce(i < 10 ? 1000 : 10);
      m.measure(() => i);
    }
    now.mockRestore();
    expect(m.count).toBe(70);
    expect(m.last).toBe(10);
    expect(m.avg).toBe(10);
  });
  it('lets an error from the measured function propagate without recording a sample', () => {
    const m = new PerfMeter();
    expect(() =>
      m.measure(() => {
        throw new Error('boom');
      }),
    ).toThrow('boom');
    expect(m.count).toBe(0);
  });
});

describe('Dashboard', () => {
  afterEach(() => {
    history.replaceState(null, '', location.pathname);
  });

  it('shows the server version, uptime and connected apps', async () => {
    const { el, live } = await mount();
    const sub = el.querySelector('.sub')!.textContent!.replace(/\s+/g, ' ');
    // The version comes from its one source (ER-13): the browser build's, which About shows too.
    expect(sub).toContain(`Pitwall ${BROWSER_VERSION} `);
    expect(BROWSER_VERSION).toMatch(/^\d+\.\d+\.\d+$/);
    expect(sub).toMatch(/up 5 h 1\d min/);
    expect(sub).toContain(`${live.apps().filter((a) => a.connected).length} apps connected`);
    expect(sub).not.toContain('Offline mode');
  });

  it('says it is connecting before the overview arrives and flags offline mode', async () => {
    const pre = await mount({ load: false });
    expect(pre.el.querySelector('.sub')!.textContent).toContain('Connecting to the server…');
    TestBed.resetTestingModule();
    const off = await mount({ patch: (o) => ({ ...o, server: { ...o.server, offline: true } }) });
    expect(off.el.querySelector('.offline')!.textContent).toBe('Offline mode');
  });

  it('has no quick-link row: the sidebar already goes everywhere', async () => {
    const { el } = await mount();
    expect(el.querySelector('nav.quick')).toBeNull();
    expect(el.querySelector('[aria-label="Quick actions"]')).toBeNull();
  });

  it('loads the 1 h history on init and draws the memory, throughput, latency and request charts', async () => {
    const { el, backend } = await mount();
    expect(el.querySelectorAll('pw-echart').length).toBe(6);
    expect(FakeChart.all.length).toBe(6);
    expect(FakeChart.all.every((c) => c.options.length >= 1)).toBe(true);
    expect(el.querySelector('pw-echart')!.getAttribute('aria-label')).toContain('RAM, VRAM and Pitwall memory');
    expect((await backend.history('1h')).range).toBe('1h');
  });

  it('shows the preset the server is on and applies another', async () => {
    const { fixture, el, live, backend, btn } = await mount();
    const radios = Array.from(el.querySelectorAll('[aria-label="Memory preset"] button'));
    expect(radios.map((r) => r.textContent!.trim())).toEqual(['Low memory', 'Balanced', 'Performance']);
    const current = live.overview()!.preset;
    expect(radios.filter((r) => r.getAttribute('aria-checked') === 'true').length).toBe(current === 'custom' ? 0 : 1);
    const spy = vi.spyOn(backend, 'applyPreset');
    btn('Low memory').click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('lowMemory');
    expect(btn('Low memory').getAttribute('aria-checked')).toBe('true');
    expect(TestBed.inject(Toasts).toasts().at(-1)?.message).toBe('Low memory preset applied.');
  });

  it('releases memory from the More actions menu and toasts the result', async () => {
    const { backend, more } = await mount();
    const spy = vi.spyOn(backend, 'releaseMemory');
    await more('Release memory');
    expect(spy).toHaveBeenCalled();
    expect(TestBed.inject(Toasts).toasts().at(-1)?.tone).toBe('ok');
  });

  it('pauses serving, shows the banner and Resume, then resumes', async () => {
    const { fixture, el, live, btn, more } = await mount();
    expect(el.querySelector('.callout')).toBeNull();
    expect(Array.from(el.querySelectorAll('.page-head button')).map((b) => b.textContent!.trim()).filter(Boolean)).toEqual(['Low memory', 'Balanced', 'Performance']);
    await more('Pause serving');
    expect(live.isBusy()('serving')).toBe(false);
    patchState(live as unknown as WritableStateSource<object>, { overview: { ...live.overview()!, server: { ...live.overview()!.server, serving: 'paused' } } });
    await settle(fixture);
    expect(el.querySelector('.callout')!.textContent).toContain('Serving is paused.');
    expect(Array.from(el.querySelectorAll('button')).some((b) => b.textContent!.includes('Pause serving'))).toBe(false);
    const spy = vi.spyOn(TestBed.inject(BACKEND), 'setServing');
    btn('Resume serving').click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith(false);
  });

  it('disables the quick actions until the overview is loaded', async () => {
    const { el, fixture } = await mount({ backend: new MockBackend({ failing: ['overview'] }) });
    (el.querySelector('button[aria-label="More actions"]') as HTMLButtonElement).click();
    await settle(fixture);
    const items = Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-item'));
    expect(items.map((i) => i.textContent!.trim())).toEqual(['Release memory', 'Pause serving']);
    expect(items.every((i) => i.disabled)).toBe(true);
    document.querySelector<HTMLElement>('.cdk-overlay-backdrop')!.click();
    for (const r of Array.from(el.querySelectorAll('[aria-label="Memory preset"] button')) as HTMLButtonElement[]) expect(r.disabled).toBe(true);
  });

  it('shows a Server error panel with Retry when the overview fails, and recovers on retry', async () => {
    const backend = new MockBackend({ failing: ['overview'] });
    const { fixture, el, live } = await mount({ backend });
    const panel = el.querySelector('pw-panel[heading="Server"]') ?? el.querySelector('pw-panel')!;
    expect(panel.querySelector('[role="alert"]')!.textContent).toContain('not responding');
    expect(el.querySelector('pw-stat-tiles')).toBeNull();
    expect(el.querySelector('pw-hot-models')).toBeNull();
    const retry = Array.from(panel.querySelectorAll('button')).find((b) => b.textContent === 'Retry')!;
    (backend as unknown as { failing: Set<string> }).failing.delete('overview');
    retry.click();
    await settle(fixture);
    expect(live.error()).toBeNull();
    expect(el.querySelector('pw-stat-tiles')).not.toBeNull();
    expect(el.querySelector('pw-hot-models')).not.toBeNull();
  });

  it('shows the loaded-models, queue and event summaries', async () => {
    const { el } = await mount();
    const subs = Array.from(el.querySelectorAll('pw-panel .titles')).map((t) => t.textContent!.replace(/\s+/g, ' ').trim());
    expect(subs.find((s) => s.startsWith('Loaded models'))).toMatch(/3 hot · \d+\.\d GB VRAM · \d+\.\d GB RAM/);
    expect(subs.find((s) => s.startsWith('Queue'))).toContain('2 running · 3 waiting');
  });

  it('has empty summaries and empty states with an empty server', async () => {
    const { el } = await mount({ backend: new MockBackend({ empty: true }) });
    const subs = Array.from(el.querySelectorAll('pw-panel .titles')).map((t) => t.textContent!.replace(/\s+/g, ' ').trim());
    expect(subs.find((s) => s.startsWith('Loaded models'))).not.toContain('hot');
    expect(subs.find((s) => s.startsWith('Queue'))).toContain('0 running · 0 waiting');
    expect(el.textContent).toContain('No model loaded');
    expect(el.textContent).toContain('No apps yet');
    expect(el.textContent).toContain('No events yet');
  });

  it('shows a pairing card for every pending request', async () => {
    const { el } = await mount();
    const cards = el.querySelectorAll('pw-pairing-card');
    expect(cards.length).toBe(1);
    expect(cards[0]!.textContent).toContain('Obsidian Copilot');
  });

  it('points at model updates in the attention area, only when there are some', async () => {
    const none = await mount();
    expect(none.el.querySelector('pw-update-banner .banner')).toBeNull();
    TestBed.resetTestingModule();
    const { el } = await mount({ backend: new MockBackend({ modelUpdates: true }) });
    const line = el.querySelector('pw-update-banner .summary')!;
    expect(line.textContent).toContain('2 model updates are available');
    expect(line.querySelector('a')!.getAttribute('href')).toBe('/models');
  });

  it('switches the history range and refetches', async () => {
    const { fixture, el, backend, btn } = await mount();
    const spy = vi.spyOn(backend, 'history');
    const radios = () => Array.from(el.querySelectorAll('[aria-label="History range"] button'));
    expect(radios().map((r) => r.textContent!.trim())).toEqual(['1h', '24h', '7d', '30d']);
    expect(radios()[0]!.getAttribute('aria-checked')).toBe('true');
    btn('7d').click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('7d');
    expect(radios()[2]!.getAttribute('aria-checked')).toBe('true');
    expect(radios()[0]!.getAttribute('aria-checked')).toBe('false');
  });

  it('replaces the chart options on a range switch and on a theme switch, merges otherwise', async () => {
    const { fixture, btn } = await mount();
    const memory = FakeChart.all[0]!;
    const replaces = () => memory.options.filter((o) => (o.opts as { notMerge: boolean }).notMerge).length;
    const before = replaces();
    btn('24h').click();
    await settle(fixture);
    await new Promise((r) => requestAnimationFrame(() => r(null)));
    expect(replaces()).toBe(before + 1);
    // The shell's Appearance setting changes the scheme; the charts follow it.
    TestBed.inject(AppearanceStore).setTheme(TestBed.inject(ThemeService).resolved() === 'dark' ? 'light' : 'dark');
    await settle(fixture);
    await new Promise((r) => requestAnimationFrame(() => r(null)));
    expect(replaces()).toBe(before + 2);
  });

  it('shows an inline error with Retry on the history charts when history fails, and retries the same range', async () => {
    const backend = new MockBackend({ failing: ['history'] });
    const { fixture, el } = await mount({ backend });
    const alerts = el.querySelectorAll('pw-panel [role="alert"]');
    expect(alerts.length).toBe(5);
    expect(Array.from(el.querySelectorAll('pw-echart')).filter((c) => !c.closest('pw-usage-card'))).toEqual([]);
    const spy = vi.spyOn(backend, 'history');
    (backend as unknown as { failing: Set<string> }).failing.delete('history');
    (alerts[0]!.querySelector('button') as HTMLButtonElement).click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('1h');
    expect(el.querySelectorAll('pw-echart').length).toBe(6);
    expect(el.querySelectorAll('pw-panel [role="alert"]').length).toBe(0);
  });

  it('names apps and models in the request charts by their display names', async () => {
    const { fixture } = await mount();
    await settle(fixture);
    await new Promise((r) => requestAnimationFrame(() => r(null)));
    const series = (i: number) => (FakeChart.all[i]!.options.at(-1)!.option as { series: { name: string }[] }).series.map((s) => s.name);
    // chart order: memory, requests by app, tokens per second, latency, requests by model
    expect(series(1)).toContain('Debrief');
    expect(series(1)).toContain('Spotter');
    // A model that is not loaded now still has its display name, from the library; never its raw id.
    expect(series(4)).toContain('Qwen3.5 9B');
    expect(series(4)).toContain('Gemma 4 12B');
    expect(series(4)).not.toContain('gemma-4-12b-it');
    // The legends are the cards' own, under the charts, in the same names.
    const legends = Array.from(fixture.nativeElement.querySelectorAll('.series-legend') as NodeListOf<HTMLElement>).map((l) => l.textContent!);
    expect(legends[0]).toContain('Nightly report script');
    expect(legends[1]).toContain('Gemma 4 12B');
  });

  it('renders the legend with theme colours and hides the perf readout by default', async () => {
    const { el } = await mount();
    expect(el.querySelector('.legend')!.textContent).toContain('RAM');
    expect(el.querySelectorAll('.legend .sw').length).toBe(3);
    expect(el.querySelector('.perf')).toBeNull();
  });

  it('shows the dev perf readout with ?perf', async () => {
    history.replaceState(null, '', '?perf');
    const { el } = await mount();
    expect(el.querySelector('.perf')!.textContent).toMatch(/charts [\d.]+ ms · avg [\d.]+ ms · \d+ builds · \d+ updates/);
  });

  it('follows live pushes without refetching the history', async () => {
    const { fixture, live, backend } = await mount();
    live.connect();
    const spy = vi.spyOn(backend, 'history');
    (backend as MockBackend).tick();
    await settle(fixture);
    expect(spy).not.toHaveBeenCalled();
    expect(live.updates()).toBeGreaterThan(0);
    live.disconnect();
  });
});
