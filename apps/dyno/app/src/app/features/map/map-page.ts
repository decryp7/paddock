import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import type { AddressClass, CountryCount } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { MapStore } from '../../state/map.store';
import { LEGEND_STEPS, arcsFor, centreOf, drawnAs, project, stepOf } from './map-geometry';
import { COUNTRY_PATHS, MAP_VIEW } from './world-map.data';

type View = 'map' | 'table';
type SortKey = 'name' | 'connections' | 'programs' | 'hosts' | 'flagged';

const CLASS_TEXT: Record<AddressClass, string> = {
  public: 'Public addresses',
  loopback: 'This computer',
  private: 'Your local network',
  linkLocal: 'Link-local addresses',
  cgnat: 'Carrier-grade NAT addresses',
  multicast: 'Multicast addresses',
  special: 'Special-purpose addresses',
  reserved: 'Reserved addresses',
};

/** The sentence for a country's focus label and tooltip. */
export function countryLabel(c: CountryCount): string {
  const n = (v: number, one: string, many: string) => `${v} ${v === 1 ? one : many}`;
  const flagged = c.flagged > 0 ? `, ${c.flagged} flagged` : '';
  const cdn = c.includesContentNetworks ? ', includes content-network addresses' : '';
  return `${c.name}: ${n(c.connections, 'connection', 'connections')} from ${n(c.programs, 'program', 'programs')}${flagged}${cdn}`;
}

/**
 * Where the connections go: a country-level world map with one great-circle arc from the user's own location to each
 * destination (the country centre), width by connection count, plus the same numbers as a table. The geometry is bundled
 * (Natural Earth, public domain): no tiles, no remote content. Locations are where an address is registered, not where a server is.
 * Never colour alone: every country with connections is focusable and named, counts are shown in the legend and on focus, and a
 * flagged country carries a marker and the word "flagged". No animation, so reduced motion needs no special case.
 */
@Component({
  selector: 'dy-map-page',
  imports: [AsyncState, Icon, MatButton, SuSegmentedControl],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Connections map</h1>
          <p>The countries your programs are connected to. Locations are where an address is registered; content networks serve from near you but are listed under one country.</p>
        </div>
      </header>

      <dy-async [loaded]="!!store.summary()" [error]="store.error()" what="the map" [rows]="5" (retry)="store.load()">
        @if (store.summary(); as s) {
          <div class="bar">
            <pd-segmented-control label="View" [options]="views" [value]="view()" (valueChange)="view.set($event === 'table' ? 'table' : 'map')" />
            <span class="faint" data-testid="home-source">{{ s.homeSource }}@if (s.homeCountry) { ({{ s.homeCountry }}) }</span>
          </div>

          @if (!s.databaseOn) {
            <p class="note" data-testid="no-database"><dy-icon name="info" /> <span>No location database yet. It downloads when you open this page if the switch in Settings is on; until then every address is shown as “location unknown”.</span></p>
          }
          @if (s.truncated > 0) {
            <p class="note" data-testid="truncated"><dy-icon name="info" /> Showing {{ s.countries.length }} countries; {{ s.truncated }} connections were left out because the system list was cut short.</p>
          }

          @if (view() === 'map') {
            <figure class="mapbox">
              <svg [attr.viewBox]="'0 0 ' + size.width + ' ' + size.height" role="group" aria-label="World map of connections by country" data-testid="world-map">
                <defs>
                  <pattern id="flagged-hatch" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="3" height="6" fill="currentColor" opacity="0.55" /></pattern>
                </defs>
                @for (p of shapes; track p.code) {
                  <path class="land" [attr.d]="p.d" [attr.data-code]="p.code" [attr.data-step]="stepFor(p.code)" />
                }
                @for (a of arcs().arcs; track a.code) {
                  <path class="arc" [attr.d]="a.d" [attr.stroke-width]="0.8 + step(a.connections) * 0.7" [attr.opacity]="0.35 + step(a.connections) * 0.15" fill="none" [attr.data-arc]="a.code" />
                }
                @if (home(); as h) {
                  <g class="home"><circle [attr.cx]="h.x" [attr.cy]="h.y" r="5" /><text [attr.x]="h.x + 8" [attr.y]="h.y + 4">You</text></g>
                }
                @for (c of s.countries; track c.code) {
                  @if (marker(c.code); as m) {
                    <g class="spot" tabindex="0" role="button" [attr.aria-label]="label(c)" [attr.data-country]="c.code" (keydown.enter)="focusInTable(c.code)" (click)="focusInTable(c.code)">
                      <title>{{ label(c) }}</title>
                      <circle [attr.cx]="m.x" [attr.cy]="m.y" [attr.r]="4 + step(c.connections) * 1.5" [attr.data-step]="step(c.connections)" />
                      @if (c.flagged > 0) { <circle [attr.cx]="m.x" [attr.cy]="m.y" [attr.r]="8 + step(c.connections)" fill="url(#flagged-hatch)" class="flag" /> }
                    </g>
                  }
                }
              </svg>
              @if (arcs().hidden > 0) { <figcaption class="faint">{{ arcs().hidden }} smaller destinations are in the table, not drawn as arcs.</figcaption> }
              <figcaption class="legend" aria-label="Legend: connections per country" data-testid="legend">
                @for (l of legend; track l.label; let i = $index) { <span class="key"><i [attr.data-step]="i"></i> {{ l.label }}</span> }
                <span class="key"><i class="hatch"></i> flagged by a list</span>
              </figcaption>
            </figure>
          } @else {
            <table class="grid" aria-label="Connections by country" data-testid="country-table">
              <thead>
                <tr>
                  @for (h of headers; track h.key) {
                    <th scope="col" [attr.aria-sort]="sort() === h.key ? (desc() ? 'descending' : 'ascending') : 'none'"><button type="button" matButton="text" (click)="sortBy(h.key)">{{ h.label }}</button></th>
                  }
                </tr>
              </thead>
              <tbody>
                @for (c of rows(); track c.code) {
                  <tr [attr.data-country]="c.code" [class.picked]="picked() === c.code">
                    <th scope="row">{{ c.name }} <span class="faint">{{ c.code }}</span>@if (c.includesContentNetworks) { <span class="faint"> · includes content networks</span> }</th>
                    <td class="num">{{ c.connections }}</td><td class="num">{{ c.programs }}</td><td class="num">{{ c.hosts }}</td>
                    <td class="num">@if (c.flagged > 0) { <b class="flagword">⚑ {{ c.flagged }} flagged</b> } @else { 0 }</td>
                  </tr>
                } @empty {
                  <tr><td colspan="5" class="empty">No connections to public addresses right now.</td></tr>
                }
              </tbody>
            </table>
          }

          <section aria-labelledby="local-h" class="block">
            <h2 id="local-h">On this computer and your network</h2>
            <ul class="local" data-testid="local-groups">
              @for (g of s.local; track g.class) { <li><b class="num">{{ g.connections }}</b> {{ g.connections === 1 ? 'connection' : 'connections' }} · {{ classText(g.class) }} · {{ g.programs }} {{ g.programs === 1 ? 'program' : 'programs' }}</li> } @empty { <li class="faint">None.</li> }
              @if (s.unknown > 0) { <li><b class="num">{{ s.unknown }}</b> {{ s.unknown === 1 ? 'connection' : 'connections' }} · location unknown</li> }
            </ul>
          </section>
          <p class="faint credit" data-testid="credit">IP Geolocation by DB-IP (https://db-ip.com), CC-BY-4.0@if (s.dbDate) { · database of {{ s.dbDate }} }. Map: Natural Earth, public domain.</p>
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-3); align-content: start; }
    dy-async { display: grid; gap: var(--sp-3); }
    .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
    .mapbox { margin: 0; display: grid; gap: var(--sp-2); }
    svg { width: 100%; height: auto; max-height: 70vh; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-md); color: var(--accent); }
    .land { fill: var(--surface-3); stroke: var(--border); stroke-width: 0.5; }
    .land[data-step='0'] { fill: color-mix(in srgb, var(--accent) 18%, var(--surface-3)); }
    .land[data-step='1'] { fill: color-mix(in srgb, var(--accent) 32%, var(--surface-3)); }
    .land[data-step='2'] { fill: color-mix(in srgb, var(--accent) 48%, var(--surface-3)); }
    .land[data-step='3'] { fill: color-mix(in srgb, var(--accent) 66%, var(--surface-3)); }
    .land[data-step='4'] { fill: color-mix(in srgb, var(--accent) 85%, var(--surface-3)); }
    .arc { stroke: var(--accent); stroke-linecap: round; }
    .spot { cursor: pointer; outline: none; }
    .spot circle { fill: var(--accent); stroke: var(--surface-1); stroke-width: 1.5; }
    .spot:focus-visible circle { stroke: var(--text-1); stroke-width: 3; }
    .spot .flag { fill: var(--warn); stroke: none; }
    .home circle { fill: var(--text-1); stroke: var(--surface-1); stroke-width: 2; }
    .home text { fill: var(--text-1); font: var(--fw-semibold) 12px var(--font-sans); paint-order: stroke; stroke: var(--surface-1); stroke-width: 3px; }
    .legend { display: flex; gap: var(--sp-3); flex-wrap: wrap; font-size: var(--fs-xs); color: var(--text-2); }
    .key { display: inline-flex; gap: 6px; align-items: center; }
    .key i { width: 14px; height: 14px; border-radius: 3px; border: 1px solid var(--border-strong); display: inline-block; }
    .key i[data-step='0'] { background: color-mix(in srgb, var(--accent) 18%, var(--surface-3)); }
    .key i[data-step='1'] { background: color-mix(in srgb, var(--accent) 32%, var(--surface-3)); }
    .key i[data-step='2'] { background: color-mix(in srgb, var(--accent) 48%, var(--surface-3)); }
    .key i[data-step='3'] { background: color-mix(in srgb, var(--accent) 66%, var(--surface-3)); }
    .key i[data-step='4'] { background: color-mix(in srgb, var(--accent) 85%, var(--surface-3)); }
    .key i.hatch { background: repeating-linear-gradient(45deg, var(--warn) 0 3px, transparent 3px 6px); }
    table.grid { border-collapse: collapse; width: 100%; font-size: var(--fs-sm); }
    .grid th, .grid td { text-align: left; padding: 6px 10px; border-bottom: 1px solid var(--border); }
    .grid thead th { color: var(--text-2); font-weight: var(--fw-semibold); padding: 0; }
    .grid td.num { text-align: right; }
    .grid tr.picked { background: var(--surface-active); }
    .flagword { color: var(--warn); }
    .block { display: grid; gap: var(--sp-1); }
    h2 { margin: 0; font-size: var(--fs-md); }
    .local { margin: 0; padding-left: var(--sp-4); font-size: var(--fs-sm); color: var(--text-2); }
    .empty { color: var(--text-2); text-align: center; padding: var(--sp-4); }
    .credit { margin: 0; }
  `,
})
export class MapPage implements OnInit, OnDestroy {
  protected readonly store = inject(MapStore);
  protected readonly size = MAP_VIEW;
  protected readonly legend = LEGEND_STEPS;
  protected readonly shapes = Object.entries(COUNTRY_PATHS).map(([code, d]) => ({ code, d }));
  protected readonly views: SuSegment[] = [
    { value: 'map', label: 'Map' },
    { value: 'table', label: 'Table' },
  ];
  protected readonly headers: { key: SortKey; label: string }[] = [
    { key: 'name', label: 'Country' },
    { key: 'connections', label: 'Connections' },
    { key: 'programs', label: 'Programs' },
    { key: 'hosts', label: 'Hosts' },
    { key: 'flagged', label: 'Flagged' },
  ];
  protected readonly view = signal<View>('map');
  protected readonly sort = signal<SortKey>('connections');
  protected readonly desc = signal(true);
  protected readonly picked = signal<string | null>(null);

  protected readonly arcs = computed(() => {
    const s = this.store.summary();
    return s ? arcsFor(s.homeCountry, s.countries) : { arcs: [], hidden: 0 };
  });
  protected readonly home = computed(() => {
    const code = this.store.summary()?.homeCountry;
    const c = code ? centreOf(code) : null;
    return c ? project(c[0], c[1]) : null;
  });
  protected readonly rows = computed(() => {
    const list = [...(this.store.summary()?.countries ?? [])];
    const k = this.sort();
    const dir = this.desc() ? -1 : 1;
    return list.sort((a, b) => (k === 'name' ? a.name.localeCompare(b.name) * dir : (a[k] - b[k]) * dir || a.name.localeCompare(b.name)));
  });

  ngOnInit(): void {
    this.store.start();
  }

  ngOnDestroy(): void {
    this.store.stop();
  }

  protected step = stepOf;
  protected label = countryLabel;
  protected classText = (c: AddressClass): string => CLASS_TEXT[c];

  /** The step (0 to 4) of a drawn country with connections, or -1. */
  protected stepFor(code: string): number {
    const c = this.store.summary()?.countries.find((x) => x.code === code);
    return c ? stepOf(c.connections) : -1;
  }

  /** Where a country's focusable marker goes: its centre on the map. */
  protected marker(code: string): { x: number; y: number } | null {
    const c = centreOf(code);
    return c && drawnAs(code) ? project(c[0], c[1]) : null;
  }

  protected sortBy(k: SortKey): void {
    if (this.sort() === k) this.desc.update((d) => !d);
    else {
      this.sort.set(k);
      this.desc.set(k !== 'name');
    }
  }

  /** Enter or click on a country shows it in the table, highlighted. */
  protected focusInTable(code: string): void {
    this.picked.set(code);
    this.view.set('table');
  }
}
