import { PdActionTip, PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent } from '@decryp7/paddock-ui/table';
import { ChangeDetectionStrategy, Component, DestroyRef, OnInit, computed, inject, signal } from '@angular/core';
import { ThemeService } from '../../core/theme';
import type { CounterHit, CounterInfo, CounterUnit, TimelineEventKind } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Chart } from '../../shared/chart';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { CountersStore, RANGES } from '../../state/counters.store';
import { SettingsStore } from '../../state/settings.store';
import { EVENT_STYLE, eventCounts } from '../dashboard/history-options';
import { EVENT_LABEL } from '../../shared/labels';
import { MAX_SET_NAME, MAX_WATCHED, counterChartOption, counterTitle, formatCounter, pathParts, readingNumber, readingReason, UNIT_WORD } from './counters-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatFormField } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';
import { FullTextTip } from '../../shared/full-text-tip';
import { SearchField } from '../../shared/search-field';
import { MatMenuModule } from '@angular/material/menu';

/**
 * Performance counters: browse what the OS offers (objects, then counters with their instances), search by name, chart
 * any of them with Dyno's own events marked, pin the ones to keep, save sets, and put a counter on the dashboard.
 * Observe-only. Counters are read only while this page is open.
 */
@Component({
  selector: 'dy-counters-page',
  imports: [AsyncState, Icon, Chart, PdTable, PdCell, PdActionTip, MatMenuModule, SearchField, FullTextTip, MatTooltip, SuSegmentedControl, PdSelect, MatFormField, MatInput, MatButton, MatIconButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Counters</h1>
          <p>The raw performance counters this computer offers. Chart any of them next to what Dyno did, and pin the ones you want on the dashboard.</p>
        </div>
      </header>

      <div class="bar" role="group" aria-label="Counters toolbar">
        <dy-search-field label="Search counters" placeholder="Search counters, for example disk queue" [value]="store.query()" (valueChange)="onSearch($event)" />
        <span class="spacer"></span>
        <button type="button" matButton="outlined" [matMenuTriggerFor]="setsMenu"><dy-icon name="layers" /> Sets @if (store.sets().length) { <span class="count num">{{ store.sets().length }}</span> }</button>
        <pd-segmented-control label="Range" [options]="rangeSegs" [value]="store.minutes() + ''" (valueChange)="store.setMinutes(+$event)" />
      </div>

      @if (naming()) {
        <div class="namebar" role="group" aria-label="Save a set">
          <mat-form-field><input matInput type="text" [attr.maxlength]="maxName" aria-label="Set name" placeholder="Name this set" [value]="setName()" (input)="setName.set($any($event.target).value)" (keydown.enter)="saveSet()" (keydown.escape)="naming.set(false)" /></mat-form-field>
          <button type="button" matButton="filled" [disabled]="!setName().trim()" (click)="saveSet()">Save set</button>
          <button type="button" matButton="text" (click)="naming.set(false)">Cancel</button>
        </div>
      }

      <div class="cols">
        <section class="card browser" aria-label="Browse counters">
          @if (store.query().trim()) {
            <div class="card-head"><h2>{{ hitsTitle() }}</h2></div>
            <dy-async [loaded]="store.hits() !== null" [error]="store.searchError()" what="search results" [rows]="6" [empty]="store.hits()?.length === 0" emptyIcon="search" emptyTitle="No counter matches" emptyText="Try fewer or different words, for example the object (memory, disk) or part of the name." (retry)="onSearch(store.query())">
              <ul class="list" role="list" aria-label="Search results">
                @for (h of store.hits(); track h.path) {
                  <li class="item" [class.on]="isWatched(h.path)">
                    <span class="name truncate" [matTooltip]="h.path">{{ hitName(h) }}</span>
                    <span class="meta truncate">{{ h.object }}</span>
                    <span class="acts" pdActionTip>
                      <button type="button" matIconButton [attr.aria-label]="(isWatched(h.path) ? 'Stop charting ' : 'Chart ') + hitName(h)" (click)="toggle(h.path)"><dy-icon [name]="isWatched(h.path) ? 'check' : 'plus'" /></button>
                    </span>
                  </li>
                }
              </ul>
            </dy-async>
          } @else if (store.opened(); as o) {
            <div class="card-head">
              <button type="button" matIconButton matTooltip="Back to all objects" aria-label="Back to all objects" (click)="store.closeObject()"><dy-icon name="chevron-left" /></button>
              <h2 class="truncate" [dyFullText]="o.name">{{ o.name }}</h2>
              <span class="spacer"></span>
              <span class="sub">{{ o.counters.length }} counters</span>
            </div>
            @if (o.instances.length) {
              <span class="inst"><span>Instance</span><pd-select [options]="instanceOptions(o.instances)" [value]="currentInstance() ?? ''" ariaLabel="Instance" (valueChange)="instance.set($event)" /></span>
              @if (o.instancesTotal > o.instances.length) {
                <p class="hint">Showing {{ o.instances.length }} of {{ o.instancesTotal }} instances. Search by name to find the rest.</p>
              }
            }
            <ul class="list" role="list" aria-label="Counters of {{ o.name }}">
              @for (c of o.counters; track c.name) {
                <li class="item" [class.on]="isWatched(pathOf(c))">
                  <span class="name truncate" [dyFullText]="c.help ?? c.name">{{ c.name }}</span>
                  <span class="meta truncate">{{ unitWord(c.unit) }}</span>
                  <span class="acts" pdActionTip>
                    <button type="button" matIconButton [attr.aria-label]="(isWatched(pathOf(c)) ? 'Stop charting ' : 'Chart ') + c.name" (click)="toggle(pathOf(c))"><dy-icon [name]="isWatched(pathOf(c)) ? 'check' : 'plus'" /></button>
                  </span>
                </li>
              }
            </ul>
          } @else {
            <div class="card-head">
              <h2>Objects</h2>
              <span class="spacer"></span>
              @if (store.objects(); as l) { <span class="sub">{{ l.objects.length }} found</span> }
            </div>
            <dy-async [loaded]="!!store.objects()" [error]="store.objectsError()" what="counter objects" [rows]="7" (retry)="store.loadObjects()">
              @if (store.openError(); as e) { <p class="hint" role="alert">{{ e.message }}</p> }
              <ul class="list" role="list" aria-label="Objects">
                @for (o of store.objects()?.objects; track o.name) {
                  <li>
                    <button type="button" class="item obj" (click)="store.open(o.name)">
                      <span class="name truncate" [dyFullText]="o.name">{{ o.name }}</span>
                      <span class="meta num">{{ o.counters }}</span>
                      <dy-icon name="chevron-right" />
                    </button>
                  </li>
                }
              </ul>
              @if (store.objects()?.truncated) { <p class="hint">The list was cut to keep it small. Search by name to find the rest.</p> }
            </dy-async>
          }
        </section>

        <div class="charts">
          <section class="card" aria-label="Chart">
            @if (!store.charted()) {
              <div class="blank">
                <dy-icon name="trend" />
                <strong>Nothing charted yet</strong>
                <span>Search for a counter or open an object, then choose + to chart it. Rates need a second reading, so lines start a moment later.</span>
              </div>
            } @else {
              @if (store.pollError(); as e) {
                <div class="error" role="alert"><dy-icon name="alert" /><span class="truncate">{{ e.message }}</span><button type="button" matButton="text" (click)="store.tick()">Retry</button></div>
              }
              @if (chart(); as c) {
                <dy-chart [options]="c.option" [label]="chartLabel()" style="--chart-h: 300px" />
                @if (c.skipped.length) {
                  <p class="hint">The chart draws two kinds of unit at a time; {{ c.skipped.length }} {{ c.skipped.length === 1 ? 'counter is' : 'counters are' }} listed below but not drawn.</p>
                }
                <div class="legend" role="group" aria-label="Events on the timeline">
                  @for (e of events(); track e.kind) {
                    <span class="chip"><span class="glyph" [style.color]="eventColor(e.kind)" aria-hidden="true">●</span> {{ eventLabel(e.kind) }} <span class="n num">{{ e.count }}</span></span>
                  } @empty {
                    <span class="faint">No alerts, restarts or network changes in this window.</span>
                  }
                </div>
              } @else {
                <div class="skeleton chart-skeleton" aria-busy="true" aria-label="Loading the chart"></div>
              }
            }
          </section>

          @if (store.charted()) {
            <section class="card" aria-label="Charted counters">
              <div class="card-head">
                <h2>Charted</h2>
                <span class="spacer"></span>
                <span class="sub">{{ store.watching().length }} of {{ maxWatched }}</span>
                <button type="button" matButton="text" [disabled]="settings.saving()" (click)="startNaming()"><dy-icon name="layers" /> Save as set</button>
                <button type="button" matButton="text" (click)="store.setWatching([])">Clear</button>
              </div>
              <div class="tablewrap">
                <pd-table label="Charted counters" [rows]="store.watching()" [columns]="columns" [rowKey]="pathKey" [filterable]="false" [rowActions]="actions" [maxHeight]="480" (action)="run($event)">
                  <ng-template pdCell="counter" let-p>
                    <span class="swatch" [style.background]="swatch(index(p))" aria-hidden="true"></span>
                    <span class="truncate">{{ title(p) }}</span>
                    @if (store.isPinned(p)) { <dy-icon class="pinned" name="pin" aria-label="Pinned" /> }
                  </ng-template>
                  <ng-template pdCell="value" let-p>
                    @if (value(p); as v) {
                      <b>{{ v[0] }}</b><small>{{ v[1] }}</small>
                    } @else if (reason(p); as why) {
                      <span class="unavail" [matTooltip]="why"><span class="dot" aria-hidden="true"></span>Unavailable</span>
                    } @else {
                      <span class="faint">…</span>
                    }
                  </ng-template>
                </pd-table>
              </div>
            </section>
          }

          @if (pinnedOutside().length) {
            <section class="card" aria-label="Pinned counters">
              <div class="card-head"><h2>Pinned</h2><span class="spacer"></span><span class="sub">{{ store.pinned().length }}</span></div>
              <ul class="list" role="list" aria-label="Pinned counters">
                @for (p of pinnedOutside(); track p) {
                  <li class="item">
                    <span class="name truncate" [matTooltip]="p">{{ title(p) }}</span>
                    <span class="meta truncate">{{ object(p) }}</span>
                    <span class="acts" pdActionTip>
                      <button type="button" matIconButton [attr.aria-label]="'Chart ' + title(p)" (click)="store.watch(p)"><dy-icon name="plus" /></button>
                      <button type="button" matIconButton [attr.aria-label]="'Unpin ' + title(p)" (click)="store.togglePin(p)"><dy-icon name="close" /></button>
                    </span>
                  </li>
                }
              </ul>
            </section>
          }
        </div>
      </div>
    </div>

    <mat-menu #setsMenu="matMenu" aria-label="Counter sets">
      <ng-template matMenuContent>
        @for (s of store.sets(); track s.name) {
          <button mat-menu-item (click)="store.applySet(s.name)"><dy-icon name="layers" /> {{ s.name }} <span class="n num">{{ s.paths.length }}</span></button>
        } @empty {
          <button mat-menu-item disabled>No saved sets yet</button>
        }
        <button mat-menu-item [disabled]="!store.charted()" (click)="startNaming()"><dy-icon name="plus" /> Save charted counters as a set…</button>
        @for (s of store.sets(); track s.name) {
          <button mat-menu-item class="danger" (click)="store.deleteSet(s.name)"><dy-icon name="trash" /> Delete “{{ s.name }}”</button>
        }
      </ng-template>
    </mat-menu>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-3); align-content: start; }
    .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; min-height: 44px; }
    .count { margin-left: 4px; padding: 0 6px; border-radius: var(--r-pill); background: var(--surface-active); color: var(--text-2); font-size: var(--fs-xs); }
    .namebar { display: flex; gap: var(--sp-2); align-items: center; }
    .cols { display: grid; grid-template-columns: 360px minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .charts { display: grid; gap: var(--sp-3); min-width: 0; }
    .card-head { padding-bottom: var(--sp-1); }
    .sub { color: var(--text-3); font-size: var(--fs-xs); }
    .truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .list { list-style: none; margin: 0; padding: 0; display: grid; }
    .item { position: relative; display: grid; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: var(--sp-2); height: 34px; padding: 0 var(--sp-3); border: 0; border-bottom: 1px solid var(--border-subtle); background: none; color: var(--text-1); font: var(--fs-sm) var(--font-sans); text-align: left; width: 100%; cursor: default; }
    .item.obj { grid-template-columns: minmax(0, 1fr) auto 16px; cursor: pointer; }
    .item:hover, .item:focus-within { background: var(--surface-hover); }
    .item.on .name { color: var(--accent-text); }
    .meta { color: var(--text-3); font-size: var(--fs-xs); max-width: 110px; }
    .item dy-icon { --icon-size: 14px; color: var(--text-3); }
    .acts { display: inline-flex; justify-content: flex-end; gap: 2px; opacity: 0.4; transition: opacity var(--dur-fast) var(--ease-out); }
    .item:hover .acts, .item:focus-within .acts, .item.on .acts { opacity: 1; }
    .ib { display: grid; place-items: center; width: 26px; height: 26px; border: 0; border-radius: var(--r-sm); background: none; color: var(--text-2); cursor: pointer; }
    .ib:hover { background: var(--surface-active); color: var(--text-1); }
    .ib dy-icon { --icon-size: 15px; color: inherit; }
    .inst { display: flex; align-items: center; gap: var(--sp-2); padding: var(--sp-1) var(--sp-3) var(--sp-2); color: var(--text-3); font-size: var(--fs-xs); }
    .inst select { flex: 1; min-width: 0; }
    .hint { margin: 0; padding: var(--sp-2) var(--sp-3); color: var(--text-3); font-size: var(--fs-xs); }
    .blank { display: grid; justify-items: center; gap: var(--sp-2); padding: var(--sp-10) var(--sp-6); text-align: center; color: var(--text-3); }
    .blank dy-icon { --icon-size: 32px; }
    .blank strong { color: var(--text-1); font-size: 15px; font-weight: var(--fw-semibold); }
    .blank span { max-width: 460px; font-size: var(--fs-sm); }
    .error { display: flex; align-items: center; gap: var(--sp-2); padding: var(--sp-2) var(--sp-3); color: var(--bad); font-size: var(--fs-sm); }
    dy-chart { padding: 0 var(--sp-3); }
    .chart-skeleton { height: 300px; margin: var(--sp-3); }
    .legend { display: flex; flex-wrap: wrap; gap: var(--sp-2); padding: var(--sp-1) var(--sp-4) var(--sp-3); }
    .chip { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 var(--sp-2); border: 1px solid var(--border); border-radius: var(--r-pill); background: var(--surface-inset); color: var(--text-2); font-size: var(--fs-xs); }
    .glyph { font-size: 10px; line-height: 1; }
    .n { color: var(--text-3); }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
    .tablewrap { margin: 0 var(--sp-3) var(--sp-3); }
    .truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    small { color: var(--text-3); margin-left: 3px; font-size: var(--fs-xs); }
    .muted { color: var(--text-2); }
    .swatch { width: 10px; height: 3px; border-radius: 2px; flex: none; }
    .pinned { --icon-size: 12px; color: var(--text-3); flex: none; }
    .unavail { display: inline-flex; align-items: center; gap: 6px; color: var(--text-2); }
    .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--warn); }
    .n { margin-left: auto; padding-left: var(--sp-3); }
    @media (max-width: 1000px) { .cols { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class CountersPage implements OnInit {
  protected readonly store = inject(CountersStore);
  protected readonly settings = inject(SettingsStore);
  private readonly destroyRef = inject(DestroyRef);
  private readonly theme = inject(ThemeService);
  private readonly notifier = inject(Notifier);
  protected readonly ranges = RANGES;
  protected readonly rangeSegs: SuSegment[] = RANGES.map((r) => ({ value: String(r.minutes), label: r.label }));
  protected readonly instanceOptions = (list: readonly string[]): PdSelectOption[] => list.map((i) => ({ value: i, label: i }));
  protected readonly maxName = MAX_SET_NAME;
  protected readonly maxWatched = MAX_WATCHED;
  protected readonly naming = signal(false);
  protected readonly setName = signal('');
  protected readonly instance = signal<string | null>(null);
  private searchTimer: ReturnType<typeof setTimeout> | null = null;

  protected readonly currentInstance = computed(() => {
    const o = this.store.opened();
    if (!o?.instances.length) return null;
    const chosen = this.instance();
    return chosen && o.instances.includes(chosen) ? chosen : (o.instances.find((i) => i.startsWith('_Total')) ?? o.instances[0]!);
  });

  protected readonly chart = computed(() => {
    const h = this.store.history();
    return h && h.series.length ? counterChartOption(h, this.theme.tokens()) : null;
  });
  protected readonly chartLabel = computed(() => `Counters over the last ${this.store.minutes()} minutes, with events marked`);
  protected readonly events = computed(() => eventCounts(this.store.history()?.events ?? []));
  protected readonly pinnedOutside = computed(() => this.store.pinned().filter((p) => !this.store.watching().includes(p)));
  protected readonly hitsTitle = computed(() => {
    const n = this.store.hits()?.length;
    return n === undefined ? 'Searching…' : `${n}${n >= 100 ? '+' : ''} ${n === 1 ? 'match' : 'matches'}`;
  });

  ngOnInit(): void {
    void this.store.loadObjects();
    // Counters are read while this page is open; the store stops reading when the last user leaves.
    const release = this.store.use();
    this.destroyRef.onDestroy(() => {
      release();
      if (this.searchTimer !== null) clearTimeout(this.searchTimer);
    });
  }

  protected onSearch(text: string): void {
    if (this.searchTimer !== null) clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => void this.store.search(text), 200);
    if (!text.trim()) void this.store.search(text);
  }

  protected hitName(h: CounterHit): string {
    return h.instance === null ? h.counter : `${h.counter} (${h.instance})`;
  }

  protected pathOf(c: CounterInfo): string {
    const o = this.store.opened();
    if (!o) return '';
    const i = this.currentInstance();
    return i === null ? `\\${o.name}\\${c.name}` : `\\${o.name}(${i})\\${c.name}`;
  }

  protected isWatched(path: string): boolean {
    return this.store.watching().includes(path);
  }

  protected toggle(path: string): void {
    void (this.isWatched(path) ? this.store.unwatch(path) : this.store.watch(path));
  }

  protected title = counterTitle;
  protected readonly pathKey = (p: string): string => p;
  protected readonly index = (p: string): number => this.store.watching().indexOf(p);
  protected readonly columns: PdColumn<string>[] = [
    { key: 'counter', header: 'Counter', value: (p) => counterTitle(p), required: true, sortable: false, width: 'minmax(0, 1.6fr)' },
    { key: 'object', header: 'Object', value: (p) => pathParts(p).object, sortable: false, width: 'minmax(0, 1fr)', hideBelow: 520 },
    { key: 'value', header: 'Value', value: (p) => this.value(p)?.join(' ') ?? '', align: 'end', sortable: false, width: '140px' },
  ];
  protected readonly actions = (p: string): PdRowAction[] => [
    { id: 'pin', label: this.store.isPinned(p) ? 'Unpin' : 'Pin', icon: 'pin', inline: true },
    { id: 'unwatch', label: 'Stop charting', icon: 'close', inline: true },
    { id: 'dashboard', label: 'Add to the dashboard', icon: 'layers' },
    { id: 'copy', label: 'Copy path', icon: 'copy' },
  ];

  protected run({ id, row }: PdRowActionEvent<string>): void {
    if (id === 'pin') this.store.togglePin(row);
    else if (id === 'unwatch') this.store.unwatch(row);
    else if (id === 'dashboard') this.store.addToDashboard(row);
    else this.copy(row);
  }
  protected unitWord = (u: CounterUnit) => UNIT_WORD[u];
  protected eventLabel = (k: TimelineEventKind) => EVENT_LABEL[k];
  protected eventColor = (k: TimelineEventKind) => EVENT_STYLE[k].color(this.theme.tokens());
  protected swatch = (i: number) => this.theme.tokens().chart[i % this.theme.tokens().chart.length]!;
  protected object = (path: string) => pathParts(path).object;

  /** The number and unit of a counter's latest reading, or null when it has none. */
  protected value(path: string): [string, string] | null {
    const r = this.store.readings()[path];
    const n = readingNumber(r);
    return r && n !== null ? formatCounter(n, r.unit) : null;
  }

  protected reason(path: string): string | null {
    return readingReason(this.store.readings()[path]);
  }

  protected startNaming(): void {
    this.setName.set('');
    this.naming.set(true);
  }

  protected async saveSet(): Promise<void> {
    if (await this.store.saveSet(this.setName())) this.naming.set(false);
  }

  protected async copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.notifier.show('Copied');
    } catch {
      this.notifier.show('Copying is not allowed here.', 'bad');
    }
  }
}
