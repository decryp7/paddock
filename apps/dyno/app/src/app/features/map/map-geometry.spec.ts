import { ISO_CODES, SMALL_PLACES } from './iso-countries';
import { arcPath, arcsFor, centreOf, drawnAs, greatCircle, project, stepOf } from './map-geometry';
import { COUNTRY_PATHS, MAP_PROJECTION, MAP_SOURCE_SHA256, MAP_VIEW } from './world-map.data';

describe('the offline world map', () => {
  it('draws every ISO code a location database can return as a shape or a dot', () => {
    const missing = ISO_CODES.filter((c) => drawnAs(c) === null);
    expect(missing).toEqual([]);
    for (const code of ISO_CODES) expect(centreOf(code), code).not.toBeNull();
  });

  it('keys shapes by ISO_A2_EH (France and Norway have shapes) and lists no code twice', () => {
    expect(COUNTRY_PATHS['FR']).toBeTruthy();
    expect(COUNTRY_PATHS['NO']).toBeTruthy();
    expect(new Set(ISO_CODES).size).toBe(ISO_CODES.length);
    expect(Object.keys(SMALL_PLACES).filter((c) => c in COUNTRY_PATHS)).toEqual([]);
  });

  it('is offline data with its provenance and a modest size', () => {
    expect(MAP_SOURCE_SHA256).toMatch(/^[0-9a-f]{64}$/);
    const bytes = JSON.stringify(COUNTRY_PATHS).length;
    expect(bytes).toBeLessThan(250 * 1024);
    expect(Object.values(COUNTRY_PATHS).every((d) => !/https?:|url\(/.test(d))).toBe(true);
  });

  it('projects with the formula the generator used: the centre meridian and equator, and the poles at the edges', () => {
    const o = project(0, 0);
    expect(o.x).toBeCloseTo(MAP_PROJECTION.centreX, 6);
    expect(o.y).toBeCloseTo(MAP_PROJECTION.equatorY, 6);
    expect(project(180, 0).x).toBeCloseTo(MAP_VIEW.width, 0);
    expect(project(-180, 0).x).toBeCloseTo(0, 0);
    expect(project(0, 90).y).toBeGreaterThan(0);
    expect(project(0, -90).y).toBeLessThan(MAP_VIEW.height);
  });

  it('puts a country centre inside its own drawn area (Germany near 10 E, 51 N)', () => {
    const [lon, lat] = centreOf('DE')!;
    expect(Math.abs(lon - 10)).toBeLessThan(4);
    expect(Math.abs(lat - 51)).toBeLessThan(3);
  });

  it('draws great-circle arcs, cut where they cross the edge of the map', () => {
    const d = arcPath(centreOf('SG')!, centreOf('US')!);
    expect(d.startsWith('M')).toBe(true);
    const segs = greatCircle(centreOf('SG')!, centreOf('US')!);
    expect(segs.length).toBeGreaterThanOrEqual(1);
    for (const s of segs) for (let i = 1; i < s.length; i++) expect(Math.abs(s[i]!.x - s[i - 1]!.x)).toBeLessThan(MAP_VIEW.width / 2);
    expect(arcPath([10, 10], [10, 10])).toBe('');
  });

  it('draws at most 250 arcs, busiest first, and says how many it left out', () => {
    const countries = ISO_CODES.filter((c) => centreOf(c)).map((code, i) => ({ code, connections: i + 1 }));
    const { arcs, hidden } = arcsFor('SG', countries, 250);
    expect(arcs.length).toBe(Math.min(250, countries.length - 1));
    expect(arcs[0]!.connections).toBeGreaterThanOrEqual(arcs[arcs.length - 1]!.connections);
    expect(hidden).toBe(Math.max(0, countries.length - 1 - 250));
    expect(arcsFor(null, countries).arcs).toEqual([]);
    expect(arcsFor('ZZ', countries).arcs).toEqual([]);
  });

  it('has the five legend steps with the numbers always shown', () => {
    expect([1, 2, 3, 9, 10, 49, 50, 199, 200, 5000].map(stepOf)).toEqual([0, 0, 1, 1, 2, 2, 3, 3, 4, 4]);
  });
});
