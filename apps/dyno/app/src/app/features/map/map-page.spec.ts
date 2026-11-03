import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import type { GeoSummary } from '../../core/api.types';
import { provideIcons } from '../../shared/icons';
import { nextFrame } from '../../testing/frame';
import { MapPage, countryLabel } from './map-page';

const NOW = 1_700_000_000_000;

async function open(patch: (s: GeoSummary) => GeoSummary = (s) => s, fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  const original = mock.geoSummary.bind(mock);
  mock.geoSummary = async () => patch(await original());
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
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
    expect(q('[data-testid="home-source"]')!.textContent).toContain('from your public IP (via Cloudflare)');
  });

  it('names every country with connections, focusable, with the numbers in its label (never colour alone)', async () => {
    const { all } = await open();
    const spots = all('g.spot');
    expect(spots.map((s) => s.dataset['country'])).toEqual(['US', 'IE', 'DE', 'JP', 'SG', 'BR']);
    for (const s of spots) {
      expect(s.getAttribute('role')).toBe('button');
      expect(s.tabIndex).toBe(0);
    }
    expect(spots[0]!.getAttribute('aria-label')).toBe('United States: 54 connections from 6 programs, 1 flagged, includes content-network addresses');
    expect(all('circle.flag')).toHaveLength(1);
  });

  it('shows the legend with the numbers, the credit and the map source', async () => {
    const { q } = await open();
    expect(q('[data-testid="legend"]')!.textContent).toContain('1–2');
    expect(q('[data-testid="legend"]')!.textContent).toContain('200+');
    expect(q('[data-testid="legend"]')!.textContent).toContain('flagged by a list');
    expect(q('[data-testid="credit"]')!.textContent).toContain('IP Geolocation by DB-IP');
    expect(q('[data-testid="credit"]')!.textContent).toContain('Natural Earth');
  });

  it('offers the same data as a table, sortable, and Enter on a country opens it there', async () => {
    const { all, q, f, settle } = await open();
    all('g.spot')[2]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await settle();
    expect(q('[data-testid="country-table"]')).toBeTruthy();
    expect(q('tr.picked')!.dataset['country']).toBe('DE');
    const rows = () => all('tbody tr').map((r) => r.dataset['country']);
    expect(rows()[0]).toBe('US');
    all('thead button')[0]!.click();
    await settle();
    expect(rows()[0]).toBe('BR');
    expect(q('thead th[aria-sort="descending"], thead th[aria-sort="ascending"]')).toBeTruthy();
    expect(f.nativeElement.textContent).toContain('flagged');
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
    expect(el.querySelector('[role="alert"]')!.textContent).toContain('geoSummary');
  });

  it('words a country label with singular and plural', () => {
    expect(countryLabel({ code: 'BR', name: 'Brazil', connections: 1, programs: 1, hosts: 1, flagged: 0, includesContentNetworks: false })).toBe('Brazil: 1 connection from 1 program');
  });
});
