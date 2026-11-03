import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import { NetworkStore } from '../../state/network.store';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import type { GeoSummary } from '../../core/api.types';
import { provideIcons } from '../../shared/icons';
import { nextFrame } from '../../testing/frame';
import { ISO_CODES } from './iso-countries';
import { MapPage, countryLabel } from './map-page';

const NOW = 1_700_000_000_000;

@Component({ template: '' })
class Stub {}

async function open(patch: (s: GeoSummary) => GeoSummary = (s) => s, fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  const original = mock.geoSummary.bind(mock);
  mock.geoSummary = async () => patch(await original());
  TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([{ path: 'network', component: Stub }]), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(MapPage);
  const settle = async () => {
    for (let i = 0; i < 4; i++) {
      await f.whenStable();
      await nextFrame();
      f.detectChanges();
    }
  };
  f.detectChanges();
  await settle();
  const el = f.nativeElement as HTMLElement;
  return { f, el, settle, q: <T extends Element = HTMLElement>(sel: string) => el.querySelector<T>(sel), all: (sel: string) => [...el.querySelectorAll<HTMLElement>(sel)] };
}

afterEach(() => TestBed.resetTestingModule());

describe('MapPage', () => {
  it('draws the bundled world, one arc per destination from the home country, and the home marker', async () => {
    const { q, all } = await open();
    expect(q('svg[data-testid="world-map"]')).toBeTruthy();
    expect(all('path.land').length).toBeGreaterThan(150);
    expect(all('path.arc').map((a) => a.dataset['arc'])).toEqual(['US', 'IE', 'DE', 'JP', 'BR']);
    expect(q('.home text')!.textContent).toBe('You');
    expect(q('[data-testid="home-source"]')!.textContent).toContain('Your location: Singapore, from your public IP (via Cloudflare)');
  });

  it('names every country with connections, focusable, with the numbers in its label (never colour alone)', async () => {
    const { all } = await open();
    const spots = all('g.spot');
    expect(spots.map((s) => s.dataset['country']).sort()).toEqual(['BR', 'DE', 'IE', 'JP', 'SG', 'US']);
    for (const s of spots) {
      expect(s.getAttribute('role')).toBe('button');
      expect(s.tabIndex).toBe(0);
    }
    expect(spots.find((s) => s.dataset['country'] === 'US')!.getAttribute('aria-label')).toBe('United States: 54 connections from 6 programs, 1 flagged by your lists, includes content-network addresses');
    // The flag is drawn (a hatched ring, a flag and the word), not only a colour.
    expect(all('circle.flag')).toHaveLength(1);
    expect(all('g.flagmark')).toHaveLength(1);
    expect(all('g.flagmark text')[0]!.textContent).toBe('flagged');
  });

  it('shows the legend with the numbers, the credit and the map source', async () => {
    const { q } = await open();
    expect(q('[data-testid="legend"]')!.textContent).toContain('1–2');
    expect(q('[data-testid="legend"]')!.textContent).toContain('200+');
    expect(q('[data-testid="legend"]')!.textContent).toContain('flagged by one of your lists');
    expect(q('[data-testid="credit"]')!.textContent).toContain('IP Geolocation by DB-IP');
    expect(q('figure [data-testid="credit"]'), 'the credit sits under the map itself').toBeTruthy();
    expect(q('[data-testid="credit"]')!.textContent).toContain('Natural Earth');
  });

  it('offers the same data as a table, sortable, with a button per country', async () => {
    const { all, q, f, settle } = await open();
    all('[role="radio"], pd-segmented-control button').find((b) => /Table/.test(b.textContent ?? ''))!.click();
    await settle();
    expect(q('[data-testid="country-table"]')).toBeTruthy();
    const rows = () => all('tbody tr').map((r) => r.dataset['country']);
    expect(rows()[0]).toBe('US');
    all('thead button')[0]!.click();
    await settle();
    expect(rows()[0]).toBe('BR');
    expect(q('thead th[aria-sort="descending"], thead th[aria-sort="ascending"]')).toBeTruthy();
    expect(f.nativeElement.textContent).toContain('flagged');
    expect(q('tr[data-country="DE"] td.act button')!.getAttribute('aria-label')).toBe('Show connections to Germany');
    expect(q('[data-testid="credit-table"]')!.textContent).toContain('DB-IP');
  });

  it('Enter or a click on a country opens the Connections table filtered to that country', async () => {
    const { all, settle } = await open();
    all('g.spot').find((s) => s.dataset['country'] === 'DE')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await settle();
    expect(TestBed.inject(NetworkStore).query().country).toBe('DE');
    expect(TestBed.inject(Router).url).toBe('/network');
  });

  it('lists the destinations that are on this computer and the unknown ones', async () => {
    const { q } = await open();
    const t = q('[data-testid="local-groups"]')!.textContent!;
    expect(t).toContain('This computer');
    expect(t).toContain('Your local network');
    expect(t).toContain('location unknown');
  });

  it('says so when no database is loaded and when connections were left out', async () => {
    const { q } = await open((s) => ({ ...s, databaseOn: false, truncated: 37, countries: [] }));
    expect(q('[data-testid="no-database"]')!.textContent).toContain('No location database yet');
    expect(q('[data-testid="truncated"]')!.textContent).toContain('37 connections were left out');
  });

  it('does not draw arcs when the home location is unknown, and says why', async () => {
    const { all, q } = await open((s) => ({ ...s, homeCountry: null, homeSource: 'Your location: unknown (no region setting found)' }));
    expect(all('path.arc')).toHaveLength(0);
    expect(q('.home')).toBeNull();
    expect(q('[data-testid="home-source"]')!.textContent).toContain('unknown (no region setting found)');
  });

  it('has no animation, so reduced motion needs no special case', async () => {
    const { el } = await open();
    expect(el.querySelector('animate, animateMotion, animateTransform')).toBeNull();
  });

  it('shows an error with Retry when the map cannot be read', async () => {
    const { el } = await open((s) => s, ['geoSummary']);
    expect(el.querySelector('[role="alert"]')!.textContent).toContain('could not read the current connections');
  });

  it('shows the country, its count and its busiest programs when a marker has focus', async () => {
    const { all, q, settle } = await open();
    all('g.spot').find((s) => s.dataset['country'] === 'US')!.dispatchEvent(new Event('focus'));
    await settle();
    const tip = q('[data-testid="country-tip"]')!;
    expect(tip.textContent).toContain('United States');
    expect(tip.textContent).toContain('54 connections');
    expect(tip.textContent).toContain('1 flagged');
    expect(tip.textContent).toContain('Google Chrome, Slack, Spotify');
    // Below the marker: the flag label sits above it.
    const spot = all('g.spot').find((s) => s.dataset['country'] === 'US')!.querySelector('circle.dot')!;
    expect(Number(spot.getAttribute('cy')) / 491 * 100).toBeLessThan(parseFloat(tip.style.top));
    all('g.spot').find((s) => s.dataset['country'] === 'US')!.dispatchEvent(new Event('blur'));
    await settle();
    expect(q('[data-testid="country-tip"]')).toBeNull();
  });

  it('keeps 250 destinations readable: arcs for the busiest few, dots for the rest, markers capped, flagged ones called out once', async () => {
    const many = (s: GeoSummary): GeoSummary => ({
      ...s,
      countries: ISO_CODES.filter((c) => c !== 'SG').map((code, i) => ({
        code,
        name: code,
        connections: ((i * 7) % 230) + 1,
        programs: 1,
        hosts: 1,
        flagged: i % 25 === 0 ? 1 : 0,
        includesContentNetworks: false,
        topPrograms: [],
      })),
    });
    const { all, q } = await open(many);
    expect(all('path.arc').length).toBe(30);
    expect(all('g.spot').length).toBeGreaterThan(200);
    const radii = all('g.spot circle.dot').map((c) => Number(c.getAttribute('r')));
    expect(Math.max(...radii)).toBeLessThanOrEqual(8);
    // More than three flagged countries: one callout in words, no per-marker word that could collide or be clipped.
    expect(all('g.flagmark text')).toHaveLength(0);
    expect(q('.callout')!.textContent).toMatch(/\d+ countries with flagged connections/);
    const rect = q('.callout rect')!;
    const text = q('.callout text')!;
    expect(text.getAttribute('text-anchor')).toBe('middle');
    expect(Number(text.getAttribute('x'))).toBeCloseTo(Number(rect.getAttribute('x')) + Number(rect.getAttribute('width')) / 2, 5);
    expect(all('circle.flag').length).toBeGreaterThan(3);
    // The home marker and the flagged markers are drawn after (above) the plain ones.
    const order = all('svg > g, svg > path').map((n) => n.getAttribute('class') ?? n.tagName);
    expect(order.lastIndexOf('home')).toBeGreaterThan(order.lastIndexOf('spot'));
  });

  it('shows a map-shaped placeholder labelled "Loading connections…" while the first answer is awaited', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    let release: (s: GeoSummary) => void = () => undefined;
    mock.geoSummary = () => new Promise<GeoSummary>((r) => (release = r));
    TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([{ path: 'network', component: Stub }]), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(MapPage);
    f.detectChanges();
    await f.whenStable();
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('[data-testid="map-loading"]')!.textContent).toContain('Loading connections…');
    expect(el.querySelector('[data-testid="map-loading"] svg path.ph')).toBeTruthy();
    release({ countries: [], local: [], unknown: 0, dbDate: null, truncated: 0, databaseOn: false, homeCountry: null, homeSource: '' });
    await f.whenStable();
    f.detectChanges();
    expect(el.querySelector('[data-testid="map-loading"]')).toBeNull();
  });

  it('keeps the credit line free of stray spaces', async () => {
    const { q } = await open();
    expect(q('[data-testid="credit"]')!.textContent).toBe('IP Geolocation by DB-IP (https://db-ip.com), CC-BY-4.0, database of 2026-11-01. Map: Natural Earth, public domain.');
  });

  it('words a country label with singular and plural', () => {
    expect(countryLabel({ code: 'BR', name: 'Brazil', connections: 1, programs: 1, hosts: 1, flagged: 0, includesContentNetworks: false, topPrograms: [] })).toBe('Brazil: 1 connection from 1 program');
  });
});
