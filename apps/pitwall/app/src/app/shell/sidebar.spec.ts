import { TestBed } from '@angular/core/testing';
import type { Download } from '../core/api.types';
import { LiveStore } from '../core/live.store';
import { MockBackend } from '../core/mock-backend';
import { ThemeService } from '../core/theme';
import { render, settle } from '../../testing/render';
import { Sidebar } from './sidebar';

const NOW = 1_800_000_000_000;

describe('Sidebar', () => {
  async function setup(backend = new MockBackend({ now: () => NOW }), load = true) {
    const r = await render(Sidebar, backend);
    const live = TestBed.inject(LiveStore);
    if (load) {
      await live.load();
      await settle(r.fixture);
    }
    return { ...r, live, theme: TestBed.inject(ThemeService) };
  }
  const links = (el: HTMLElement) => Array.from(el.querySelectorAll<HTMLAnchorElement>('nav a.item'));

  beforeEach(() => localStorage.clear());

  it('lists the screens in groups', async () => {
    const { el } = await setup();
    expect(links(el).map((a) => a.querySelector('.label')?.textContent)).toEqual(['Dashboard', 'Models', 'Engines', 'Apps & keys', 'Requests', 'Playground', 'Benchmarks']);
    expect(Array.from(el.querySelectorAll('.group')).map((g) => g.textContent)).toEqual(['Serve', 'Try']);
    expect(links(el).map((a) => a.getAttribute('href'))).toEqual(['/', '/models', '/engines', '/apps', '/requests', '/playground', '/benchmarks']);
    expect(el.querySelector('a.settings')?.getAttribute('href')).toBe('/settings');
  });

  it('shows connecting, then the serving state with the port', async () => {
    const { el, fixture, live } = await setup(new MockBackend({ now: () => NOW }), false);
    expect(el.querySelector('.state')?.textContent).toContain('Connecting');
    await live.load();
    await settle(fixture);
    expect(el.querySelector('.state')?.textContent?.replace(/\s+/g, ' ')).toContain('Serving · :11500');
    expect(el.querySelector('.state')?.getAttribute('data-state')).toBe('serving');
  });

  it('shows paused when serving is paused', async () => {
    const { el, fixture, live } = await setup();
    live.connect();
    await live.setServing(true);
    await settle(fixture);
    expect(el.querySelector('.state')?.textContent).toContain('Paused');
    expect(el.querySelector('.state')?.getAttribute('data-state')).toBe('paused');
  });

  it('says the server is unreachable when the overview fails', async () => {
    const { el } = await setup(new MockBackend({ failing: ['overview'] }));
    expect(el.querySelector('.state')?.textContent).toContain('Server unreachable');
    expect(el.querySelector('.state')?.getAttribute('data-state')).toBe('down');
    expect(el.querySelector('.mem')).toBeNull();
  });

  it('badges pending pairing requests on Apps and active downloads on Models', async () => {
    const be = new MockBackend({ now: () => NOW });
    const download = { id: 'd1', state: 'downloading' } as Download;
    const done = { id: 'd2', state: 'done' } as unknown as Download;
    be.downloads = () => Promise.resolve([download, done]);
    const { el } = await setup(be);
    const pairings = TestBed.inject(LiveStore).pairings().length;
    expect(pairings).toBeGreaterThan(0);
    const apps = links(el).find((a) => a.textContent?.includes('Apps'))!;
    expect(apps.querySelector('.badge.attention')?.textContent).toBe(String(pairings));
    expect(apps.querySelector('.badge')?.getAttribute('aria-label')).toBe(`${pairings} pairing requests`);
    const models = links(el).find((a) => a.textContent?.includes('Models'))!;
    expect(models.querySelector('.badge')?.textContent?.trim()).toBe('1');
    expect(models.querySelector('.badge')?.getAttribute('aria-label')).toBe('1 downloads running');
  });

  it('has no badges on an empty install', async () => {
    const { el } = await setup(new MockBackend({ empty: true }));
    expect(el.querySelectorAll('.badge')).toHaveLength(0);
  });

  it('shows memory use with meters and the tone thresholds', async () => {
    const { el, fixture, live } = await setup();
    const m = live.memory()!;
    expect(el.querySelector('.mem')?.getAttribute('aria-label')).toMatch(/^Memory: \d+ percent used$/);
    expect(el.querySelectorAll('.meter').length).toBeGreaterThanOrEqual(1);
    expect(el.querySelector('.mem-row .num')?.textContent).toContain('/');
    expect(m.ramTotal).toBeGreaterThan(0);

    for (const [used, tone] of [[0.97, 'bad'], [0.9, 'warn'], [0.3, 'neutral']] as const) {
      live.disconnect();
      const be = new MockBackend({ now: () => NOW });
      const ov = await be.overview();
      ov.memory.ramAvailable = Math.round(ov.memory.ramTotal * (1 - used));
      be.overview = () => Promise.resolve(structuredClone(ov));
      TestBed.resetTestingModule();
      const r = await render(Sidebar, be);
      await TestBed.inject(LiveStore).load();
      await settle(r.fixture);
      expect(r.el.querySelector('.meter')?.getAttribute('data-tone')).toBe(tone);
    }
    expect(fixture).toBeTruthy();
  });

  it('shows VRAM only when a GPU reports it', async () => {
    const withGpu = await setup();
    expect(withGpu.el.textContent).toContain('VRAM');
    TestBed.resetTestingModule();
    const be = new MockBackend({ now: () => NOW });
    const ov = await be.overview();
    ov.memory.gpus = [];
    be.overview = () => Promise.resolve(structuredClone(ov));
    const none = await setup(be);
    expect(none.el.textContent).not.toContain('VRAM');
    expect(none.el.querySelectorAll('.meter')).toHaveLength(1);
  });

  it('shows VRAM text without a meter when usage is unknown', async () => {
    const be = new MockBackend({ now: () => NOW });
    const ov = await be.overview();
    ov.memory.gpus = [{ ...ov.memory.gpus[0]!, vramUsed: null }];
    be.overview = () => Promise.resolve(structuredClone(ov));
    const { el } = await setup(be);
    expect(el.textContent).not.toContain('VRAM');
  });

  it('switches the theme with the radio buttons and reflects the choice', async () => {
    const { el, fixture, theme } = await setup();
    const radios = Array.from(el.querySelectorAll<HTMLButtonElement>('.theme button'));
    expect(radios.map((b) => b.getAttribute('aria-label'))).toEqual(['System theme', 'Light theme', 'Dark theme']);
    expect(radios[0]!.getAttribute('aria-checked')).toBe('true');
    radios[2]!.click();
    await settle(fixture);
    expect(theme.choice()).toBe('dark');
    expect(radios[2]!.getAttribute('aria-checked')).toBe('true');
    expect(radios[0]!.getAttribute('aria-checked')).toBe('false');
    radios[1]!.click();
    await settle(fixture);
    expect(theme.choice()).toBe('light');
  });
});
