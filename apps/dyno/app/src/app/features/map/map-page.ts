import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { MatButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import type { AddressClass, CountryCount } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { MapStore } from '../../state/map.store';
import { NetworkStore } from '../../state/network.store';
import { LEGEND_STEPS, OUTLINE_CODES, arcsFor, spherePath, centreOf, drawnAs, project, stepOf, worldPaths } from './map-geometry';
import { MAP_VIEW } from './world-map.data';

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
  const flagged = c.flagged > 0 ? `, ${c.flagged} flagged by your lists` : '';
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

      <dy-async [loaded]="!!store.summary()" [error]="store.error()" what="the map" [rows]="0" (retry)="store.load()">
        <div skeleton class="map-skeleton" role="status" aria-live="polite" data-testid="map-loading">
          <svg [attr.viewBox]="'0 0 ' + size.width + ' ' + size.height" aria-hidden="true"><path class="ph" [attr.d]="sphere()" /></svg>
          <span>Loading connections…</span>
        </div>
        @if (store.summary(); as s) {
          <div class="bar">
            <pd-segmented-control label="View" [options]="views" [value]="view()" (valueChange)="view.set($event === 'table' ? 'table' : 'map')" />
            <span class="faint" data-testid="home-source">{{ homeText(s) }}</span>
          </div>

          @if (!s.databaseOn) {
            <p class="note" data-testid="no-database"><dy-icon name="info" /> <span>No location database yet. It downloads when you open this page if the switch in Settings is on; until then every address is shown as “location unknown”.</span></p>
          }
          @if (s.truncated > 0) {
            <p class="note" data-testid="truncated"><dy-icon name="info" /> Showing {{ s.countries.length }} countries; {{ s.truncated }} connections were left out because the system list was cut short.</p>
          }

          @if (view() === 'map') {
            <figure class="mapbox">
              <div class="frame">
                <svg [attr.viewBox]="'0 0 ' + size.width + ' ' + size.height" role="group" aria-label="World map of connections by country" data-testid="world-map">
                  <defs>
                    <pattern id="flagged-hatch" width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="2.2" height="5" class="hatch-bar" /></pattern>
                    <clipPath id="sphere"><path [attr.d]="sphere()" /></clipPath>
                  </defs>
                  <path class="sea" [attr.d]="sphere()" />
                  <g clip-path="url(#sphere)">
                    @for (p of shapes(); track p.code) {
                      <path class="land" [attr.d]="p.d" [attr.data-code]="p.code" [attr.data-step]="stepFor(p.code)" />
                    }
                    @for (a of arcs().arcs; track a.code) {
                      <path class="arc" [attr.d]="a.d" [attr.stroke-width]="1.4 + step(a.connections) * 0.7" [attr.opacity]="0.55 + step(a.connections) * 0.1" fill="none" [attr.data-arc]="a.code" />
                    }
                  </g>
                  @for (c of plain(); track c.code) {
                    @if (marker(c.code); as m) {
                      <g class="spot" tabindex="0" role="button" [attr.aria-label]="label(c)" [attr.data-country]="c.code" (keydown.enter)="showConnections(c.code)" (click)="showConnections(c.code)" (focus)="tip.set(c.code)" (blur)="tip.set(null)" (mouseenter)="tip.set(c.code)" (mouseleave)="tip.set(null)">
                        <circle class="dot" [attr.cx]="m.x" [attr.cy]="m.y" [attr.r]="dotRadius(c.connections)" [attr.data-step]="step(c.connections)" />
                      </g>
                    }
                  }
                  @for (c of flaggedCountries(); track c.code) {
                    @if (marker(c.code); as m) {
                      <g class="spot" tabindex="0" role="button" [attr.aria-label]="label(c)" [attr.data-country]="c.code" (keydown.enter)="showConnections(c.code)" (click)="showConnections(c.code)" (focus)="tip.set(c.code)" (blur)="tip.set(null)" (mouseenter)="tip.set(c.code)" (mouseleave)="tip.set(null)">
                        <circle [attr.cx]="m.x" [attr.cy]="m.y" [attr.r]="dotRadius(c.connections) + 5" fill="url(#flagged-hatch)" class="flag" />
                        <g class="flagmark" [attr.transform]="'translate(' + (m.x + (m.x > size.width - 70 ? -9 : 7) ) + ' ' + (m.y - 20) + ')'">
                          <path d="M0 0v14M0 1h9l-2.5 3.5L9 8H0" />
                          @if (flaggedCountries().length <= 3) {
                            <rect class="pill" [attr.x]="m.x > size.width - 70 ? -58 : 11" y="-2" width="50" height="16" rx="4" />
                            <text [attr.x]="m.x > size.width - 70 ? -54 : 15" y="10">flagged</text>
                          }
                        </g>
                        <circle class="dot" [attr.cx]="m.x" [attr.cy]="m.y" [attr.r]="dotRadius(c.connections)" [attr.data-step]="step(c.connections)" />
                      </g>
                    }
                  }
                  @if (flaggedCountries().length > 3) {
                    <g class="callout"><rect class="pill" x="10" y="10" [attr.width]="calloutWidth()" height="22" rx="5" /><text [attr.x]="10 + calloutWidth() / 2" y="25" text-anchor="middle">{{ calloutText() }}</text></g>
                  }
                  @if (home(); as h) {
                    <g class="home"><circle [attr.cx]="h.x" [attr.cy]="h.y" r="6" /><rect class="pill" [attr.x]="h.x + 8" [attr.y]="h.y - 8" width="30" height="16" rx="4" /><text [attr.x]="h.x + 12" [attr.y]="h.y + 4">You</text></g>
                  }
                </svg>
                @if (tipFor(); as t) {
                  <div class="tip" role="status" data-testid="country-tip" [style.left.%]="t.left" [style.top.%]="t.top">
                    <strong>{{ t.country.name }}</strong>
                    <span>{{ t.country.connections }} {{ t.country.connections === 1 ? 'connection' : 'connections' }}@if (t.country.flagged > 0) { · {{ t.country.flagged }} flagged }</span>
                    <span class="faint" data-testid="tip-step">Legend step {{ legend[step(t.country.connections)]!.label }}</span>
                    @if (t.country.topPrograms.length > 0) { <span class="faint">{{ t.country.topPrograms.join(', ') }}</span> }
                  </div>
                }
              </div>
              <figcaption class="faint credit" data-testid="credit">{{ credit(s) }}</figcaption>
              @if (arcs().hidden > 0) { <figcaption class="faint">Arcs are drawn for the {{ arcs().arcs.length }} busiest destinations; {{ arcs().hidden }} more are dots. The table lists every country.</figcaption> }
              <figcaption class="legend" aria-label="Legend: connections per country" data-testid="legend">
                @for (l of legend; track l.label; let i = $index) {
                  <span class="key" [attr.data-step]="i">
                    <svg class="swatch" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" [attr.data-step]="i"><rect x="0.5" y="0.5" width="15" height="15" rx="3" /></svg>
                    <svg class="size" viewBox="0 0 18 16" width="18" height="16" aria-hidden="true"><circle cx="9" cy="8" [attr.r]="dotRadius(l.min)" /></svg>
                    {{ l.label }}
                  </span>
                }
                <span class="key"><i class="hatch"></i> ⚑ flagged by one of your lists</span>
              </figcaption>
            </figure>
          } @else {
            <table class="grid" aria-label="Connections by country" data-testid="country-table">
              <thead>
                <tr>
                  @for (h of headers; track h.key) {
                    <th scope="col" [attr.aria-sort]="sort() === h.key ? (desc() ? 'descending' : 'ascending') : 'none'"><button type="button" matButton="text" (click)="sortBy(h.key)">{{ h.label }}</button></th>
                  }
                  <th scope="col" class="act"><span class="sr-only">Connections</span></th>
                </tr>
              </thead>
              <tbody>
                @for (c of rows(); track c.code) {
                  <tr [attr.data-country]="c.code" [class.picked]="picked() === c.code">
                    <th scope="row">{{ c.name }} <span class="faint">{{ c.code }}</span>@if (c.includesContentNetworks) { <span class="faint"> · includes content networks</span> }</th>
                    <td class="num">{{ c.connections }}</td><td class="num">{{ c.programs }}</td><td class="num">{{ c.hosts }}</td>
                    <td class="num">@if (c.flagged > 0) { <b class="flagword">⚑ {{ c.flagged }} flagged</b> } @else { 0 }</td>
                    <td class="act"><button type="button" matButton="text" class="show" (click)="showConnections(c.code)" [attr.aria-label]="'Show connections to ' + c.name">Show connections</button></td>
                  </tr>
                } @empty {
                  <tr><td colspan="6" class="empty">No connections to public addresses right now.</td></tr>
                }
              </tbody>
            </table>
            <p class="faint credit" data-testid="credit-table">{{ credit(s) }}</p>
          }

          <section aria-labelledby="local-h" class="block">
            <h2 id="local-h">On this computer and your network</h2>
            <ul class="local" data-testid="local-groups">
              @for (g of s.local; track g.class) { <li><b class="num">{{ g.connections }}</b> {{ g.connections === 1 ? 'connection' : 'connections' }} · {{ classText(g.class) }} · {{ g.programs }} {{ g.programs === 1 ? 'program' : 'programs' }}</li> } @empty { <li class="faint">None.</li> }
              @if (s.unknown > 0) { <li><b class="num">{{ s.unknown }}</b> {{ s.unknown === 1 ? 'connection' : 'connections' }} · location unknown</li> }
            </ul>
          </section>

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
    .frame { position: relative; }
    .map-skeleton { position: relative; }
    .map-skeleton svg { display: block; width: 100%; height: auto; max-height: 70vh; }
    .map-skeleton .ph { fill: var(--surface-active); stroke: var(--border-strong); stroke-width: 1; }
    .map-skeleton span { position: absolute; inset: 0; display: grid; place-items: center; color: var(--text-2); font-size: var(--fs-md); }
    svg { display: block; width: 100%; height: auto; max-height: 70vh; color: var(--accent); }
    .hatch-bar { fill: var(--warn); }
    .tip { position: absolute; display: grid; gap: 2px; padding: 6px 10px; border: 1px solid var(--border-strong); border-radius: var(--r-sm); background: var(--surface-2); color: var(--text-1); font-size: var(--fs-xs); box-shadow: 0 2px 8px rgb(0 0 0 / 25%); pointer-events: none; max-width: 240px; }
    .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    .act { text-align: right; white-space: nowrap; }
    /* The sea is the page's own surface and the land one step away from it (1.5 to 1); the five count steps are the accent's count
       ramp (--pd-count-1 to -5), told apart by lightness in every colour vision (the token test measures it). A spec measures the
       sea, land and lowest step in both themes. */
    :host { --land-mix: 17%; --sea: var(--surface-1); --land: color-mix(in srgb, var(--text-1) var(--land-mix), var(--surface-1)); }
    @media (prefers-color-scheme: light) {
      :host-context(:root:not([data-theme='dark'])) { --land-mix: 20%; }
    }
    :host-context(:root[data-theme='light']) { --land-mix: 20%; }
    .land { fill: var(--land); stroke: var(--sea); stroke-width: 0.4; }
    .land[data-step='0'] { fill: var(--pd-count-1); }
    .land[data-step='1'] { fill: var(--pd-count-2); }
    .land[data-step='2'] { fill: var(--pd-count-3); }
    .land[data-step='3'] { fill: var(--pd-count-4); }
    .land[data-step='4'] { fill: var(--pd-count-5); }
    .key .swatch rect { stroke: var(--border-strong); stroke-width: 1; }
    .key .swatch[data-step='0'] rect { fill: var(--pd-count-1); }
    .key .swatch[data-step='1'] rect { fill: var(--pd-count-2); }
    .key .swatch[data-step='2'] rect { fill: var(--pd-count-3); }
    .key .swatch[data-step='3'] rect { fill: var(--pd-count-4); }
    .key .swatch[data-step='4'] rect { fill: var(--pd-count-5); }
    .key .size circle { fill: var(--accent); stroke: var(--surface-1); stroke-width: 1.5; }
    .sea { fill: var(--sea); stroke: var(--border); stroke-width: 1; }
    .arc { stroke: var(--accent); stroke-linecap: round; }
    .callout text { fill: var(--text-1); font: var(--fw-semibold) 12px var(--font-sans); }
    .spot { cursor: pointer; outline: none; }
    .spot .dot { fill: var(--accent); stroke: var(--surface-1); stroke-width: 1.5; }
    .spot:focus-visible .dot { stroke: var(--text-1); stroke-width: 3; }
    .spot .flag { stroke: var(--warn); stroke-width: 1.5; color: var(--warn); }
    .flagmark path { fill: var(--warn); stroke: var(--warn); stroke-width: 1.4; stroke-linejoin: round; }
    .flagmark text { fill: var(--text-1); font: var(--fw-semibold) 11px var(--font-sans); }
    .home circle { fill: var(--text-1); stroke: var(--surface-1); stroke-width: 2; }
    .pill { fill: var(--surface-1); stroke: var(--border-strong); stroke-width: 0.8; }
    .home text { fill: var(--text-1); font: var(--fw-semibold) 12px var(--font-sans); }
    .legend { display: flex; gap: var(--sp-3); flex-wrap: wrap; font-size: var(--fs-xs); color: var(--text-2); }
    .key { display: inline-flex; gap: 6px; align-items: center; }
    .key svg { display: block; width: auto; height: 16px; }
    .key i.hatch { width: 14px; height: 14px; border-radius: 3px; border: 1px solid var(--border-strong); display: inline-block; background: repeating-linear-gradient(45deg, var(--warn) 0 3px, transparent 3px 6px); }
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
  private readonly network = inject(NetworkStore);
  private readonly router = inject(Router);
  protected readonly size = MAP_VIEW;
  protected readonly legend = LEGEND_STEPS;
  /** The longitude in the middle of the map: the user's own, so a route from home stays whole across the Pacific. */
  protected readonly centre = computed(() => {
    const code = this.store.summary()?.homeCountry;
    return (code ? centreOf(code)?.[0] : null) ?? 0;
  });
  protected readonly shapes = computed(() => {
    const paths = worldPaths(this.centre());
    return OUTLINE_CODES.map((code) => ({ code, d: paths[code] ?? '' }));
  });
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
  protected readonly tip = signal<string | null>(null);
  protected readonly calloutText = computed(() => `⚑ ${this.flaggedCountries().length} countries with flagged connections`);
  /** The callout is as wide as its text plus the same padding on both sides. */
  protected readonly calloutWidth = computed(() => Math.round(this.calloutText().length * 6.6) + 24);
  protected readonly sphere = computed(() => spherePath(this.centre()));
  protected readonly flaggedCountries = computed(() => (this.store.summary()?.countries ?? []).filter((c) => c.flagged > 0));
  protected readonly plain = computed(() => (this.store.summary()?.countries ?? []).filter((c) => c.flagged === 0));
  /** A marker's radius, capped so a busy country does not cover its neighbours. */
  protected dotRadius = (connections: number): number => 3 + stepOf(connections);
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
    return c ? project(c[0], c[1], this.centre()) : null;
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
  /** The data credit: DB-IP's licence asks for it, Natural Earth does not but gets it. */
  protected credit(s: { dbDate: string | null }): string {
    return `IP Geolocation by DB-IP (https://db-ip.com), CC-BY-4.0${s.dbDate ? `, database of ${s.dbDate}` : ''}. Map: Natural Earth, public domain.`;
  }

  protected classText = (c: AddressClass): string => CLASS_TEXT[c];

  /** "Your location: Singapore, from your public IP (via Cloudflare)": the country is named in words, not only coded. */
  protected homeText(s: { homeCountry: string | null; homeSource: string }): string {
    if (!s.homeCountry) return s.homeSource;
    let name = s.homeCountry;
    try {
      name = new Intl.DisplayNames(['en'], { type: 'region' }).of(s.homeCountry) ?? name;
    } catch {
      // keep the code
    }
    return s.homeSource.replace(/^Your location:\s*/, `Your location: ${name}, `);
  }

  /** The tooltip of the focused or hovered country: where it sits (in percent of the map: below the marker, so the flag label above it stays readable) and its numbers. */
  protected readonly tipFor = computed(() => {
    const code = this.tip();
    const country = this.store.summary()?.countries.find((c) => c.code === code);
    const m = code ? this.marker(code) : null;
    return country && m ? { country, left: Math.min(78, (m.x / this.size.width) * 100), top: Math.min(88, (m.y / this.size.height) * 100 + 4) } : null;
  });

  /** The step (0 to 4) of a drawn country with connections, or -1. */
  protected stepFor(code: string): number {
    const c = this.store.summary()?.countries.find((x) => x.code === code);
    return c ? stepOf(c.connections) : -1;
  }

  /** Where a country's focusable marker goes: its centre on the map. */
  protected marker(code: string): { x: number; y: number } | null {
    const c = centreOf(code);
    return c && drawnAs(code) ? project(c[0], c[1], this.centre()) : null;
  }

  protected sortBy(k: SortKey): void {
    if (this.sort() === k) this.desc.update((d) => !d);
    else {
      this.sort.set(k);
      this.desc.set(k !== 'name');
    }
  }

  /** Enter or a click on a country opens the Connections table filtered to it (the engine names each connection's country). */
  protected showConnections(code: string): void {
    this.picked.set(code);
    void this.network.setQuery({ country: code }).then(() => this.router.navigateByUrl('/network'));
  }
}
