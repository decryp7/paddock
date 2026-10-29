import { NgTemplateOutlet } from '@angular/common';
import { PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent, type PdRowTree } from '@decryp7/paddock-ui/table';
import { ChangeDetectionStrategy, Component, DestroyRef, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import type { ConnectionRow, ListenerRow, NetProto } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatRate, formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { NetworkStore } from '../../state/network.store';
import { Lookups } from '../lookup/lookups';
import { type GroupBy, type TableRow, connectionLine, endpoint, exposureBadge, initial, listenerLine, lookupBlockedReason, middleEllipsis, ownerName, ownerReason, portService, protoText, reachTone, sortListeners, stateView, summarise, tableRows } from './network-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';
import { SearchField } from '../../shared/search-field';
import { MatCheckbox } from '@angular/material/checkbox';
import { MatMenuModule } from '@angular/material/menu';

type Tab = 'connections' | 'listening';
type Row = TableRow<ConnectionRow> | TableRow<ListenerRow>;

const COLLAPSED_KEY = 'dyno.networkCollapsed';

function readCollapsed(): Set<string> {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]');
    return new Set(Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);
  } catch {
    return new Set();
  }
}

/**
 * Open ports and connections as one dense table (one sticky header, virtualised rows), grouped by program or remote host
 * with collapsible group rows, a summary strip above and a detail panel beside it. Observe-only: Dyno lists, it never
 * closes a connection. Loopback is hidden by default; "Exposed" marks listeners reachable from the network; "Look up IP"
 * opens the user's lookup page in the browser for public peers.
 */
@Component({
  selector: 'dy-network-page',
  imports: [AsyncState, Icon, NgTemplateOutlet, PdTable, PdCell, MatMenuModule, MatCheckbox, MatIconButton, SearchField, MatTooltip, SuSegmentedControl, MatButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Network</h1>
          <p>Which programs listen for connections and who they talk to. Dyno only looks: it never closes a connection or changes a firewall.</p>
        </div>
      </header>

      @if (store.snapshot(); as s) {
        <p class="summary num" role="group" aria-label="Summary">
          @if (tab() === 'connections') {
            <button type="button" matButton="text" class="dy-link lnk" (click)="showConnections()"><b>{{ sum().connections }}</b> connections</button><span>·</span><span><b>{{ sum().programs }}</b> programs</span><span>·</span><span><b>{{ sum().hosts }}</b> remote hosts</span>
          } @else {
            <span><b>{{ sum().listening }}</b> listening</span><span>·</span><button type="button" matButton="text" class="dy-link lnk" (click)="showExposed()"><b>{{ sum().exposed }}</b> exposed</button>@if (sum().unknown > 0) {<span>·</span><span><b>{{ sum().unknown }}</b> unknown owner</span>}
          }
          @if (thisPc(); as ip) { <span class="this-pc">This PC {{ ip }}</span> }
        </p>
        @if (s.trafficNote; as why) { <p class="faint traffic-note">{{ why }}</p> }
      }

      <div class="bar" role="group" aria-label="Filters">
        <pd-segmented-control label="Show" [options]="tabSegs" [value]="tab()" (valueChange)="setTab($event === 'listening' ? 'listening' : 'connections')" />
        <dy-search-field label="Search the network table" placeholder="Port, address or program" [value]="store.query().search" (valueChange)="search($event)" />
        <mat-checkbox [checked]="store.query().showLoopback" (change)="store.setQuery({ showLoopback: !store.query().showLoopback })">Show loopback</mat-checkbox>
        <mat-checkbox [checked]="store.query().exposedOnly" (change)="store.setQuery({ exposedOnly: !store.query().exposedOnly })">Exposed only</mat-checkbox>
        <pd-segmented-control label="Protocol" [options]="protoSegs" [value]="store.query().proto ?? ''" (valueChange)="proto($event)" />
        <pd-segmented-control label="Group by" [options]="groupSegs()" [value]="group()" (valueChange)="setGroup($event)" />
        @if (tab() === 'connections') {
          <mat-checkbox [checked]="store.resolveNames()" (change)="store.setResolveNames(!store.resolveNames())" matTooltip="Look up host names through your DNS resolver. Off by default: each address is sent to it.">Host names</mat-checkbox>
        }
      </div>

      <dy-async [loaded]="!!store.snapshot()" [error]="store.error()" what="network" [rows]="5" (retry)="store.load()">
        @if (store.snapshot(); as s) {
          @if (s.unknownOwners > 0 && !noteDismissed()) {
            <p class="note"><dy-icon name="info" /> <span>{{ s.unknownOwners }} {{ s.unknownOwners === 1 ? 'socket has' : 'sockets have' }} an owner the OS will not name without elevated rights.</span> <button type="button" matIconButton matTooltip="Dismiss this note" aria-label="Dismiss this note" (click)="noteDismissed.set(true)"><dy-icon name="close" /></button></p>
          }
          @if (s.truncated > 0) {
            <p class="note"><dy-icon name="info" /> {{ s.truncated }} more rows are hidden. Narrow the search.</p>
          }
          <div class="layout" [class.with-detail]="!!selectedItem()">
            <pd-table
              [label]="tab() === 'connections' ? 'Connections' : 'Listening ports'"
              order="source"
              [tree]="tree()"
              [rows]="rows()"
              [columns]="columns()"
              [rowKey]="rowKey"
              [filterable]="false"
              [countLabel]="noCount"
              [rowHeight]="36"
              [maxHeight]="bodyMax()"
              [rowActions]="actions"
              [emptyText]="emptyText()"
              [(selectedKey)]="selectedKey"
              (rowActivate)="activate($event)"
              (rowToggle)="toggle($event.key)"
              (action)="run($event)"
              (keydown)="key($event)"
            >
              <ng-template pdCell="prog" let-r>
                @if (r.kind === 'group') {
                  <ng-container *ngTemplateOutlet="groupCell; context: { $implicit: r }" />
                } @else {
                  <span class="av" aria-hidden="true">{{ ini(name(r.item.owner)) }}</span><span class="truncate" [title]="reason(r.item.owner) ?? ''">{{ name(r.item.owner) }}</span>
                }
              </ng-template>
              <ng-template pdCell="remote" let-r>
                @if (r.kind === 'group') {
                  <ng-container *ngTemplateOutlet="groupCell; context: { $implicit: r }" />
                } @else {
                  <span class="truncate" [title]="r.item.remote">{{ short(r.item.remote) }}</span><span class="addr">:{{ r.item.remotePort }}</span>
                }
              </ng-template>
              <ng-template pdCell="service" let-r>
                @if (r.kind === 'group') {
                  <ng-container *ngTemplateOutlet="groupCell; context: { $implicit: r }" />
                } @else {
                  {{ service(tab() === 'connections' ? r.item.remotePort : r.item.port) }}
                }
              </ng-template>
              <ng-template pdCell="host" let-r>@if (r.kind === 'item') {<span class="truncate" [title]="hostName(r.item) ?? ''">{{ hostName(r.item) ?? (r.item.lookupOk ? '—' : 'Private address') }}</span>}</ng-template>
              <ng-template pdCell="state" let-r>@if (r.kind === 'item') {<span class="dot" [attr.data-tone]="state(r.item).tone" aria-hidden="true"></span>{{ state(r.item).text }}}</ng-template>
              <ng-template pdCell="address" let-r>
                @if (r.kind === 'item') {
                <span class="addr">{{ r.item.local }}</span>
                <span class="reach" [attr.data-tone]="reachTone(r.item)" [title]="badge(r.item).hint"><dy-icon [name]="r.item.exposure === 'allInterfaces' ? 'globe' : r.item.exposure === 'loopback' ? 'lock' : 'network'" /> {{ badge(r.item).label }}</span>
                }
              </ng-template>
            </pd-table>

            <ng-template #groupCell let-r>
              <span class="gname">
                <button type="button" class="chev" tabindex="-1" [attr.aria-label]="(r.open ? 'Collapse ' : 'Expand ') + r.title" (click)="toggle(r.key, $event)"><dy-icon [name]="r.open ? 'chevron-down' : 'chevron-right'" /></button>
                @if (group() === 'process') { <span class="av" aria-hidden="true">{{ ini(r.title) }}</span> }
                <strong class="truncate">{{ r.title }}</strong>
                <span class="meta">{{ r.count }} {{ tab() === 'connections' ? (r.count === 1 ? 'connection' : 'connections') : (r.count === 1 ? 'port' : 'ports') }}@if (tab() === 'connections' && group() === 'process') { · {{ r.hosts }} {{ r.hosts === 1 ? 'host' : 'hosts' }} }@if (r.exposed) { · <span class="exp">{{ r.exposed }} exposed</span> }</span>
                @if (tab() === 'connections' && group() === 'process' && trafficOf(r.title); as tr) { <span class="traffic num" title="Bytes per second over the last few seconds">↓ {{ rate(tr.rxBps) }} · ↑ {{ rate(tr.txBps) }}</span> }
              </span>
            </ng-template>

            @if (selectedItem(); as sel) {
              <aside class="detail" aria-label="Details">
                <div class="dhead">
                  <strong>{{ detailTitle(sel) }}</strong>
                  <button type="button" matIconButton matTooltip="Close details" aria-label="Close details" (click)="selectedKey.set(null)"><dy-icon name="close" /></button>
                </div>
                <dl>
                  @for (f of detailFields(sel); track f[0]) {
                    <dt>{{ f[0] }}</dt>
                    <dd class="sel">{{ f[1] }}</dd>
                  }
                </dl>
                @if (isConn(sel)) {
                  @let c3 = asConn(sel);
                  <div class="dacts">
                    <button type="button" matButton="outlined" [disabled]="!!blocked(c3)" [matTooltip]="blocked(c3) ?? null" (click)="lookup(c3)"><dy-icon name="external" /> Look up IP</button>
                    <button type="button" matButton="outlined" (click)="copyText(connectionLine(c3))"><dy-icon name="copy" /> Copy</button>
                  </div>
                  @if (blocked(c3); as why) { <p class="hint">{{ why }}</p> }
                  <div class="dacts">
                    <button type="button" matButton="outlined" [disabled]="!c3.lookupOk" (click)="store.resolveOne(c3.remote)"><dy-icon name="globe" /> Resolve name</button>
                  </div>
                  @if (hostName(c3); as hn) { <p class="hint sel">Host name: {{ hn }}</p> }
                } @else {
                  @let l3 = asListener(sel);
                  <p class="hint">{{ badge(l3).hint }}</p>
                  <div class="dacts"><button type="button" matButton="outlined" (click)="copyText(listenerLine(l3))"><dy-icon name="copy" /> Copy</button></div>
                }
              </aside>
            }
          </div>
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .chev { width: 22px; height: 22px; padding: 0; flex: none; --icon-size: 14px; }
    .page { display: grid; gap: var(--sp-3); align-content: start; }
    dy-async { display: grid; gap: var(--sp-3); }
    .summary { margin: 0; display: flex; gap: var(--sp-2); align-items: baseline; flex-wrap: wrap; color: var(--text-2); font-size: var(--fs-sm); }
    .summary b { color: var(--text-1); font-weight: var(--fw-semibold); }
    .summary .this-pc { margin-left: auto; color: var(--text-3); }
    .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; }
    .traffic { margin-left: auto; color: var(--text-3); font-size: var(--fs-xs); white-space: nowrap; }
    .reach { display: inline-flex; align-items: center; gap: 4px; color: var(--text-2); font-size: var(--fs-xs); white-space: nowrap; }
    .reach dy-icon { --icon-size: 13px; }
    .reach[data-tone='warn'] { color: var(--warn); }
    .chk { display: inline-flex; gap: var(--sp-2); align-items: center; font-size: var(--fs-sm); color: var(--text-2); }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); }
    .layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .layout.with-detail { grid-template-columns: minmax(0, 1fr) 320px; }
    .muted { color: var(--text-2); }
    .truncate { overflow: hidden; text-overflow: ellipsis; }
    .addr { font: var(--fw-regular) var(--fs-xs) / 1 var(--font-mono); color: var(--text-3); }
    .gname { display: flex; flex: 1; align-items: center; gap: var(--sp-2); min-width: 0; }
    .meta { color: var(--text-3); font-size: var(--fs-xs); white-space: nowrap; }
    .exp { color: var(--warn); }
    .av { flex: none; display: grid; place-items: center; width: 20px; height: 20px; border-radius: 5px; background: var(--surface-active); color: var(--text-2); font: var(--fw-semibold) 11px / 1 var(--font-sans); }
    .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--border-strong); flex: none; }
    .dot[data-tone='ok'] { background: var(--ok); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .ib { display: grid; place-items: center; width: 24px; height: 24px; border: 0; border-radius: var(--r-sm); background: none; color: var(--text-2); cursor: pointer; }
    .ib:hover:not(:disabled) { background: var(--surface-active); color: var(--text-1); }
    .ib:disabled { opacity: 0.35; cursor: default; }
    .ib dy-icon { --icon-size: 15px; }
    .detail { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-3) var(--sp-4); display: grid; gap: var(--sp-2); }
    .dhead { display: flex; justify-content: space-between; align-items: center; gap: var(--sp-2); }
    dl { margin: 0; display: grid; grid-template-columns: 92px minmax(0, 1fr); gap: 6px var(--sp-3); font-size: var(--fs-sm); }
    dt { color: var(--text-3); }
    dd { margin: 0; overflow-wrap: anywhere; }
    dd.sel { user-select: text; cursor: text; }
    .dacts { display: flex; gap: var(--sp-2); margin-top: var(--sp-1); }
    .hint { margin: 0; color: var(--text-3); font-size: var(--fs-xs); }
    @media (max-width: 1100px) { .layout.with-detail { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class NetworkPage implements OnInit, OnDestroy {
  protected readonly store = inject(NetworkStore);
  private readonly notifier = inject(Notifier);
  protected readonly tab = signal<Tab>('connections');
  protected readonly group = signal<GroupBy>('process');
  protected readonly tabSegs: SuSegment[] = [{ value: 'connections', label: 'Connections' }, { value: 'listening', label: 'Listening' }];
  protected readonly protoSegs: SuSegment[] = [{ value: '', label: 'All' }, { value: 'tcp', label: 'TCP' }, { value: 'udp', label: 'UDP' }];
  protected readonly groupSegs = computed<SuSegment[]>(() => [{ value: 'none', label: 'None' }, { value: 'process', label: 'Program' }, ...(this.tab() === 'connections' ? [{ value: 'host', label: 'Remote host' }] : [])]);
  protected readonly showLocal = signal(false);
  protected readonly collapsed = signal<ReadonlySet<string>>(readCollapsed());
  protected readonly selectedKey = signal<string | null>(null);
  /** The machine's own address as the connections see it (constant, so it is shown once here rather than on every row). */
  protected readonly thisPc = computed(() => this.connections().find((c) => c.local !== '127.0.0.1' && c.local !== '::1' && !c.local.startsWith('fe80'))?.local ?? null);
  private readonly router = inject(Router);
  private readonly lookups = inject(Lookups);
  private readonly destroyRef = inject(DestroyRef);
  protected readonly trafficByName = computed(() => new Map((this.store.snapshot()?.traffic ?? []).map((t) => [t.name, t])));
  protected readonly trafficOf = (title: string) => this.trafficByName().get(title) ?? null;
  protected readonly rate = formatRate;
  protected readonly hostName = (c: ConnectionRow): string | null => this.store.names()[c.remote] ?? null;
  protected readonly noteDismissed = signal(false);
  protected readonly short = (a: string) => middleEllipsis(a);

  protected readonly connections = computed<ConnectionRow[]>(() => this.store.snapshot()?.connections ?? []);
  protected readonly listeners = computed<ListenerRow[]>(() => sortListeners(this.store.snapshot()?.listeners ?? []));
  protected readonly sum = computed(() => summarise(this.connections(), this.listeners()));
  protected readonly rows = computed<Row[]>(() => {
    const by = this.tab() === 'listening' && this.group() === 'host' ? 'none' : this.group();
    return this.tab() === 'connections'
      ? tableRows(this.connections(), by, this.collapsed())
      : tableRows(this.listeners(), by, this.collapsed(), (l) => l.exposure === 'allInterfaces');
  });
  /** The columns follow the tab, the grouping and the options; a group's heading spans the row, from the first column. */
  protected readonly columns = computed<PdColumn<Row>[]>(() => {
    const prog: PdColumn<Row>[] = this.group() !== 'process' ? [{ key: 'prog', header: 'Program', value: (r) => this.itemOwner(r), width: 'minmax(130px, 1.2fr)', sortable: false }] : [];
    if (this.tab() === 'connections') {
      return [
        ...prog,
        { key: 'remote', header: 'Remote', value: (r) => this.itemRemote(r), required: true, width: 'minmax(160px, 2fr)', sortable: false },
        ...(this.store.resolveNames() ? [{ key: 'host', header: 'Host name', value: (r: Row) => (r.kind === 'item' ? (this.hostName(r.item as ConnectionRow) ?? '') : ''), width: 'minmax(140px, 2fr)', sortable: false }] : []),
        { key: 'service', header: 'Service', value: (r) => this.itemService(r), width: 'minmax(90px, 1fr)', sortable: false },
        { key: 'proto', header: 'Protocol', value: (r) => (r.kind === 'item' ? protoText((r.item as ConnectionRow).proto, (r.item as ConnectionRow).ipv6) : ''), width: '80px', sortable: false },
        { key: 'state', header: 'State', value: (r) => (r.kind === 'item' ? stateView((r.item as ConnectionRow).state).text : ''), width: '110px', sortable: false },
        ...(this.showLocal() ? [{ key: 'local', header: 'Local', value: (r: Row) => (r.kind === 'item' ? endpoint((r.item as ConnectionRow).local, (r.item as ConnectionRow).localPort) : ''), width: 'minmax(150px, 1.2fr)', sortable: false }] : []),
        { key: 'since', header: 'Since', value: (r) => this.itemSince(r), text: (r) => (r.kind === 'item' ? this.since(this.itemSince(r)) : ''), width: '84px', sortable: false },
      ];
    }
    return [
      ...prog,
      { key: 'service', header: 'Service', value: (r) => this.itemService(r), required: true, width: 'minmax(240px, 2fr)', sortable: false },
      { key: 'address', header: 'Address', value: (r) => (r.kind === 'item' ? (r.item as ListenerRow).local : ''), width: 'minmax(220px, 2fr)', sortable: false },
      { key: 'proto', header: 'Protocol', value: (r) => (r.kind === 'item' ? protoText((r.item as ListenerRow).proto, (r.item as ListenerRow).ipv6) : ''), width: '80px', sortable: false },
      { key: 'since', header: 'Since', value: (r) => this.itemSince(r), text: (r) => (r.kind === 'item' ? this.since(this.itemSince(r)) : ''), width: '90px', sortable: false },
    ];
  });
  private readonly groupedTree: PdRowTree<Row> = { level: (r) => (r.kind === 'group' ? 1 : 2), expanded: (r) => (r.kind === 'group' ? r.open : null), spans: (r) => r.kind === 'group' };
  /** Programs or remote hosts: groups at level 1, their sockets below. Ungrouped, the table is a plain grid. */
  protected readonly tree = computed(() => (this.group() === 'none' || (this.tab() === 'listening' && this.group() === 'host') ? null : this.groupedTree));
  private readonly viewportHeight = signal(typeof window === 'undefined' ? 900 : window.innerHeight);
  /** The rows scroll inside the table, which takes what the window leaves under the filters and the summary. */
  protected readonly bodyMax = computed(() => Math.max(240, this.viewportHeight() - 300));
  protected readonly noCount = (): string => '';
  protected readonly emptyText = computed(() => (this.tab() === 'connections' ? 'No connections with these filters.' : 'Nothing is listening with these filters.'));
  protected readonly rowKey = (r: Row): string => r.key;
  private itemOwner(r: Row): string {
    return r.kind === 'item' ? ownerName((r.item as ConnectionRow | ListenerRow).owner) : r.title;
  }
  private itemRemote(r: Row): string {
    return r.kind === 'item' ? (r.item as ConnectionRow).remote : r.title;
  }
  private itemService(r: Row): string {
    if (r.kind === 'group') return r.title;
    return portService(this.isConn(r.item as ConnectionRow | ListenerRow) ? (r.item as ConnectionRow).remotePort : (r.item as ListenerRow).port);
  }
  private itemSince(r: Row): number | null {
    return r.kind === 'item' ? (r.item as ConnectionRow | ListenerRow).firstSeen : null;
  }
  protected readonly selectedItem = computed(() => {
    const k = this.selectedKey();
    const r = this.rows().find((x) => x.key === k);
    return r && r.kind === 'item' ? (r.item as ConnectionRow | ListenerRow) : null;
  });

  protected readonly name = ownerName;
  protected readonly reason = ownerReason;
  protected readonly ini = initial;
  protected readonly service = portService;
  protected readonly protoText = protoText;
  protected readonly ep = endpoint;
  protected readonly connectionLine = connectionLine;
  protected readonly listenerLine = listenerLine;
  protected readonly blocked = lookupBlockedReason;
  protected readonly state = (c: ConnectionRow) => stateView(c.state);
  protected readonly badge = (l: ListenerRow) => exposureBadge(l.exposure);
  protected readonly reachTone = reachTone;
  protected readonly conn = (r: Row) => (r as { item: ConnectionRow }).item;
  protected readonly lis = (r: Row) => (r as { item: ListenerRow }).item;
  protected readonly isConn = (x: ConnectionRow | ListenerRow): x is ConnectionRow => 'remote' in x;
  protected readonly asConn = (x: ConnectionRow | ListenerRow) => x as ConnectionRow;
  protected readonly asListener = (x: ConnectionRow | ListenerRow) => x as ListenerRow;
  private searchTimer: ReturnType<typeof setTimeout> | null = null;

  ngOnInit(): void {
    const onResize = () => this.viewportHeight.set(window.innerHeight);
    window.addEventListener('resize', onResize);
    this.destroyRef.onDestroy(() => window.removeEventListener('resize', onResize));
    void this.store.start();
  }

  ngOnDestroy(): void {
    this.store.stop();
    if (this.searchTimer !== null) clearTimeout(this.searchTimer);
  }

  protected setTab(t: Tab): void {
    this.tab.set(t);
    this.selectedKey.set(null);
    if (t === 'listening' && this.group() === 'host') this.group.set('process');
  }

  protected showConnections(): void {
    this.setTab('connections');
    void this.store.setQuery({ exposedOnly: false });
  }

  protected showExposed(): void {
    this.setTab('listening');
    void this.store.setQuery({ exposedOnly: true });
  }

  protected setGroup(g: string): void {
    this.group.set(g === 'host' || g === 'none' ? g : 'process');
    this.selectedKey.set(null);
  }

  protected search(text: string): void {
    if (this.searchTimer !== null) clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => void this.store.setQuery({ search: text }), 200);
  }

  protected proto(v: string): void {
    void this.store.setQuery({ proto: v === 'tcp' || v === 'udp' ? (v as NetProto) : null });
  }

  protected since(at: number | null): string {
    return at === null ? '—' : formatRelative(at, this.store.snapshot()?.at ?? at);
  }

  protected toggle(key: string, e?: Event): void {
    e?.stopPropagation();
    const title = key.startsWith('g:') ? key.slice(2) : key;
    const next = new Set(this.collapsed());
    if (!next.delete(title)) next.add(title);
    this.collapsed.set(next);
    try {
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next].slice(0, 500)));
    } catch {
      // Storage may be blocked: the state is then only kept for this visit.
    }
  }

  /** A row was opened (a click, Enter or Space): a group folds, anything else is the one the side panel shows. */
  protected activate(r: Row): void {
    if (r.kind === 'group') this.toggle(r.key);
  }

  /** The row actions of a socket (a group heading has none): look up and copy on hover, the rest in the menu. */
  protected readonly actions = (r: Row): PdRowAction[] => {
    if (r.kind !== 'item') return [];
    const item = r.item as ConnectionRow | ListenerRow;
    const process = [
      { id: 'process', label: 'Show process', icon: 'activity' },
      ...(item.owner.kind === 'process' ? [{ id: 'web', label: 'Search the web…', icon: 'search' }] : []),
      ...(item.owner.kind === 'process' && item.owner.path ? [{ id: 'hash', label: 'Search file hash on VirusTotal…', icon: 'shield' }] : []),
    ];
    if (!this.isConn(item)) return [{ id: 'copyrow', label: 'Copy as text', icon: 'copy', inline: true }, { id: 'copy', label: 'Copy listener', icon: 'copy' }, ...process];
    return [
      { id: 'lookup', label: 'Look up IP', icon: 'external', inline: true, disabled: !!lookupBlockedReason(item) },
      { id: 'copyrow', label: 'Copy as text', icon: 'copy', inline: true },
      { id: 'resolve', label: 'Resolve host name', icon: 'globe', disabled: !item.lookupOk },
      { id: 'copy', label: 'Copy address', icon: 'copy' },
      { id: 'filter', label: 'Filter to this host', icon: 'search' },
      ...process,
    ];
  };

  protected run({ id, row }: PdRowActionEvent<Row>): void {
    if (row.kind !== 'item') return;
    const item = row.item as ConnectionRow | ListenerRow;
    this.selectedKey.set(row.key);
    if (id === 'lookup' && this.isConn(item)) this.lookup(item);
    else if (id === 'copyrow') void this.copyRow(row);
    else if (id === 'resolve' && this.isConn(item)) void this.store.resolveOne(item.remote);
    else if (id === 'copy') void this.copyText(this.isConn(item) ? item.remote : listenerLine(item));
    else if (id === 'filter' && this.isConn(item)) void this.store.setQuery({ search: item.remote });
    else if (id === 'process') this.showProcess();
    else if (id === 'web') this.searchWeb(ownerName(item.owner));
    else if (id === 'hash' && item.owner.kind === 'process' && item.owner.path) this.searchHash(item.owner.path, item.owner.name);
  }

  /** Escape closes the side panel; Ctrl or Cmd+C copies the focused socket (unless text is selected). */
  protected key(e: KeyboardEvent): void {
    if (e.key === 'Escape' && this.selectedKey() !== null) {
      this.selectedKey.set(null);
      e.preventDefault();
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'c' && !window.getSelection()?.toString()) {
      const focused = e.target instanceof Element ? e.target.closest<HTMLElement>('[role="row"][data-key]') : null;
      const row = this.rows().find((r) => r.key === focused?.dataset['key']);
      if (row?.kind === 'item') {
        e.preventDefault();
        void this.copyRow(row);
      }
    }
  }

  protected searchWeb(name: string): void {
    void this.lookups.searchWeb(name, name);
  }

  protected searchHash(path: string, name: string): void {
    void this.lookups.searchHash({ path }, name);
  }

  protected showProcess(): void {
    void this.router.navigateByUrl('/processes');
  }

  protected lookup(c: ConnectionRow, e?: Event): void {
    e?.stopPropagation();
    void this.store.lookup(c.remote);
  }

  protected copyRow(r: Row, e?: Event): Promise<void> {
    e?.stopPropagation();
    if (r.kind !== 'item') return Promise.resolve();
    const item = r.item as ConnectionRow | ListenerRow;
    return this.copyText(this.isConn(item) ? connectionLine(item) : listenerLine(item));
  }

  protected async copyText(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.notifier.show('Copied');
    } catch {
      this.notifier.show('Copying is not allowed here.', 'bad');
    }
  }

  protected detailTitle(x: ConnectionRow | ListenerRow): string {
    return this.isConn(x) ? `${ownerName(x.owner)} → ${x.remote}` : `${ownerName(x.owner)} · port ${x.port.toString()}`;
  }

  protected detailFields(x: ConnectionRow | ListenerRow): [string, string][] {
    const owner: [string, string][] =
      x.owner.kind === 'process'
        ? [['Program', x.owner.name], ['Process id', x.owner.pid.toString()], ['Path', x.owner.path ?? 'Not readable']]
        : [['Program', 'Owner unknown'], ['Why', x.owner.reason]];
    const seen: [string, string][] = [['First seen', this.since(x.firstSeen)]];
    if (this.isConn(x)) {
      return [
        ...owner,
        ['Remote', endpoint(x.remote, x.remotePort)],
        ...(this.hostName(x) ? ([['Host name', this.hostName(x) as string]] as [string, string][]) : []),
        ['Service', portService(x.remotePort)],
        ['Local', endpoint(x.local, x.localPort)],
        ['Protocol', protoText(x.proto, x.ipv6)],
        ['State', stateView(x.state).text],
        ...seen,
      ];
    }
    return [...owner, ['Address', endpoint(x.local, x.port)], ['Service', portService(x.port)], ['Protocol', protoText(x.proto, x.ipv6)], ['Reach', exposureBadge(x.exposure).label], ...seen];
  }
}
