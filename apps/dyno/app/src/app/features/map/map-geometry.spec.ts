import { ISO_CODES, SMALL_PLACES } from './iso-countries';
import { EQUATOR_Y, OUTLINE_CODES, arcPath, arcsFor, centreOf, countryPath, drawnAs, greatCircle, project, stepOf, worldPaths } from './map-geometry';
import { COUNTRY_RINGS, MAP_SOURCE_SHA256, MAP_VIEW } from './world-map.data';

describe('the offline world map', () => {
  it('draws every ISO code a location database can return as a shape or a dot', () => {
    const missing = ISO_CODES.filter((c) => drawnAs(c) === null);
    expect(missing).toEqual([]);
    for (const code of ISO_CODES) expect(centreOf(code), code).not.toBeNull();
  });

  it('keys shapes by ISO_A2_EH (France and Norway have shapes) and lists no code twice', () => {
    expect(COUNTRY_RINGS['FR']).toBeTruthy();
    expect(COUNTRY_RINGS['NO']).toBeTruthy();
    expect(new Set(ISO_CODES).size).toBe(ISO_CODES.length);
    expect(Object.keys(SMALL_PLACES).filter((c) => c in COUNTRY_RINGS)).toEqual([]);
  });

  it('is offline data with its provenance and a modest size', () => {
    expect(MAP_SOURCE_SHA256).toMatch(/^[0-9a-f]{64}$/);
    const bytes = JSON.stringify(COUNTRY_RINGS).length;
    expect(bytes).toBeLessThan(250 * 1024);
    expect(Object.values(COUNTRY_RINGS).every((d) => !/https?:|url\(/.test(d))).toBe(true);
  });

  it('projects Equal Earth with the chosen longitude in the middle and the poles at the edges', () => {
    const o = project(0, 0);
    expect(o.x).toBeCloseTo(MAP_VIEW.width / 2, 6);
    expect(o.y).toBeCloseTo(EQUATOR_Y, 6);
    expect(project(180, 0).x).toBeCloseTo(0, 0);
    expect(project(104, 1.35, 104).x).toBeCloseTo(MAP_VIEW.width / 2, 6);
    expect(project(104 + 179, 0, 104).x).toBeCloseTo(MAP_VIEW.width, -1);
    expect(project(0, 90).y).toBeGreaterThan(0);
    expect(project(0, -90).y).toBeLessThan(MAP_VIEW.height);
  });

  it('centres the outlines on a longitude: the same country moves, and each outline is drawn three times so nothing is lost at the edge', () => {
    const at0 = countryPath('DE', 0);
    const at104 = countryPath('DE', 104);
    expect(at0).not.toBe(at104);
    expect((at104.match(/Z/g) ?? []).length % 3).toBe(0);
    expect(OUTLINE_CODES.length).toBeGreaterThan(150);
    expect(worldPaths(104.4)).toBe(worldPaths(104));
    expect(Object.keys(worldPaths(0)).sort()).toEqual([...OUTLINE_CODES].sort());
  });

  it('keeps the route from Singapore to the United States whole across the Pacific when the map is centred on Singapore', () => {
    const sg = centreOf('SG')!;
    const us = centreOf('US')!;
    const centred = greatCircle(sg, us, 32, sg[0]);
    expect(centred).toHaveLength(1);
    expect(centred[0]!.length).toBe(33);
    for (const p of centred[0]!) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(MAP_VIEW.width);
    }
    // Centred on Greenwich the same route is cut at the edge of the frame.
    expect(greatCircle(sg, us, 32, 0).length).toBeGreaterThan(1);
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
