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
import { registerIcons } from '../../shared/icons';
import { FakeChart, fakeChartProvider, settle } from '../../../testing/render';
import { Dashboard, PerfMeter, shortModel } from './dashboard';

async function mount(opts: { backend?: MockBackend; load?: boolean; patch?: (o: Overview) => Overview } = {}) {
  const backend = opts.backend ?? new MockBackend();
  FakeChart.all = [];
  TestBed.configureTestingModule({ imports: [Dashboard], providers: [{ provide: BACKEND, useValue: backend }, provideRouter([]), fakeChartProvider] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  if (opts.load !== false) await live.load();
  if (opts.patch) patchState(live as unknown as WritableStateSource<object>, { overview: opts.patch(live.overview()!) });
  const fixture = TestBed.createComponent(Dashboard);
  await settle(fixture);
  const el = fixture.nativeElement as HTMLElement;
  const btn = (label: string) => Array.from(el.querySelectorAll('button')).find((b) => b.textContent!.trim() === label || b.textContent!.includes(label)) as HTMLButtonElement;
  return { fixture, el, live, backend, btn };
}

describe('shortModel', () => {
  it('drops a trailing quantisation suffix', () => {
    expect(shortModel('gemma-3-12b-it-q4_k_m')).toBe('gemma-3-12b-it');
    expect(shortModel('llama-3.2-3b-instruct-Q5_K_M')).toBe('llama-3.2-3b-instruct');
    expect(shortModel('nomic-embed-text-v1.5-f16')).toBe('nomic-embed-text-v1.5');
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
    expect(sub).toContain('Pitwall 0.1.0');
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

  it('links to the main places', async () => {
    const { el } = await mount();
    const links = Array.from(el.querySelectorAll('nav.quick a')).map((a) => [a.textContent!.trim(), a.getAttribute('href')]);
    expect(links).toEqual([
      ['Open Models', '/models'],
      ['Pair an app', '/apps'],
      ['Playground', '/playground'],
      ['Requests', '/requests'],
    ]);
  });

  it('loads the 1 h history on init and draws the memory, throughput, latency and request charts', async () => {
    const { el, backend } = await mount();
    expect(el.querySelectorAll('pw-echart').length).toBe(5);
    expect(FakeChart.all.length).toBe(5);
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

  it('releases memory and toasts the result', async () => {
    const { fixture, backend, btn } = await mount();
    const spy = vi.spyOn(backend, 'releaseMemory');
    btn('Release memory').click();
    await settle(fixture);
    expect(spy).toHaveBeenCalled();
    expect(TestBed.inject(Toasts).toasts().at(-1)?.tone).toBe('ok');
  });

  it('pauses serving, shows the banner and Resume, then resumes', async () => {
    const { fixture, el, live, btn } = await mount();
    expect(el.querySelector('.banner')).toBeNull();
    btn('Pause serving').click();
    await settle(fixture);
    expect(live.isBusy()('serving')).toBe(false);
    patchState(live as unknown as WritableStateSource<object>, { overview: { ...live.overview()!, server: { ...live.overview()!.server, serving: 'paused' } } });
    await settle(fixture);
    expect(el.querySelector('.banner')!.textContent).toContain('Serving is paused.');
    expect(Array.from(el.querySelectorAll('button')).some((b) => b.textContent!.includes('Pause serving'))).toBe(false);
    const spy = vi.spyOn(TestBed.inject(BACKEND), 'setServing');
    btn('Resume serving').click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith(false);
  });

  it('disables the quick actions until the overview is loaded', async () => {
    const { el, btn } = await mount({ backend: new MockBackend({ failing: ['overview'] }) });
    expect(btn('Release memory').disabled).toBe(true);
    expect(btn('Pause serving').disabled).toBe(true);
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
    TestBed.inject(ThemeService).set(TestBed.inject(ThemeService).resolved() === 'dark' ? 'light' : 'dark');
    await settle(fixture);
    await new Promise((r) => requestAnimationFrame(() => r(null)));
    expect(replaces()).toBe(before + 2);
  });

  it('shows an inline error with Retry on the history charts when history fails, and retries the same range', async () => {
    const backend = new MockBackend({ failing: ['history'] });
    const { fixture, el } = await mount({ backend });
    const alerts = el.querySelectorAll('pw-panel [role="alert"]');
    expect(alerts.length).toBe(5);
    expect(el.querySelector('pw-echart')).toBeNull();
    const spy = vi.spyOn(backend, 'history');
    (backend as unknown as { failing: Set<string> }).failing.delete('history');
    (alerts[0]!.querySelector('button') as HTMLButtonElement).click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('1h');
    expect(el.querySelectorAll('pw-echart').length).toBe(5);
    expect(el.querySelectorAll('pw-panel [role="alert"]').length).toBe(0);
  });

  it('names apps and models in the request charts, falling back to a shortened id for unloaded ones', async () => {
    const { fixture } = await mount();
    await settle(fixture);
    await new Promise((r) => requestAnimationFrame(() => r(null)));
    const legend = (i: number) => ((FakeChart.all[i]!.options.at(-1)!.option as { legend: { data: string[] } }).legend.data);
    // chart order: memory, requests by app, tokens per second, latency, requests by model
    expect(legend(1)).toContain('IMT');
    expect(legend(1)).toContain('Spotter');
    expect(legend(4)).toContain('Qwen3 8B Instruct');
    expect(legend(4)).toContain('gemma-3-12b-it');
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
