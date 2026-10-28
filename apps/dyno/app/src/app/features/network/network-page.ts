import { CdkFixedSizeVirtualScroll, CdkVirtualForOf, CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, signal, viewChild } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import type { ConnectionRow, ListenerRow, NetProto } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatRate, formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { NetworkStore } from '../../state/network.store';
import { Lookups } from '../lookup/lookups';
import {
  type GroupBy,
  type TableRow,
  connectionLine,
  endpoint,
  exposureBadge,
  initial,
  listenerLine,
  lookupBlockedReason,
  middleEllipsis,
  ownerName,
  ownerReason,
  portService,
  protoText,
  reachTone,
  sortListeners,
  stateView,
  summarise,
  tableRows,
} from './network-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';
import { SearchField } from '../../shared/search-field';
import { MatCheckbox } from '@angular/material/checkbox';
import { MatMenuModule } from '@angular/material/menu';
import { openContextMenu } from '../../shared/context-menu';

type Tab = 'connections' | 'listening';
type Row = TableRow<ConnectionRow> | TableRow<ListenerRow>;

const COLLAPSED_KEY = 'dyno.networkCollapsed';
const ROW_H = 36;

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
  imports: [MatMenuModule, MatCheckbox, MatIconButton, SearchField, MatTooltip, SuSegmentedControl, MatButton, AsyncState, Icon, RouterLink, CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf],
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
            <p class="note"><dy-icon name="info" /> <span>{{ s.unknownOwners }} {{ s.unknownOwners === 1 ? 'socket has' : 'sockets have' }} an owner the OS will not name without elevated rights.</span> <button type="button" matIconButton aria-label="Dismiss this note" (click)="noteDismissed.set(true)"><dy-icon name="close" /></button></p>
          }
          @if (s.truncated > 0) {
            <p class="note"><dy-icon name="info" /> {{ s.truncated }} more rows are hidden. Narrow the search.</p>
          }
          <div class="layout" [class.with-detail]="!!selectedItem()">
            <div class="grid" role="grid" [attr.aria-label]="tab() === 'connections' ? 'Connections' : 'Listening ports'" [attr.aria-rowcount]="rows().length" tabindex="0" (keydown)="key($event)" [style.--cols]="cols()">
              <div class="head" role="row">
                @if (tab() === 'connections') {
                  @if (group() !== 'process') { <span role="columnheader">Program</span> }
                  <span role="columnheader">Remote</span>
                  @if (store.resolveNames()) { <span role="columnheader">Host name</span> }
                  <span role="columnheader">Service</span>
                  <span role="columnheader">Protocol</span>
                  <span role="columnheader">State</span>
                  @if (showLocal()) { <span role="columnheader">Local</span> }
                  <span role="columnheader">Since</span>
                } @else {
                  @if (group() !== 'process') { <span role="columnheader">Program</span> }
                  <span role="columnheader">Service</span>
                  <span role="columnheader">Address</span>
                  <span role="columnheader">Protocol</span>
                  <span role="columnheader">Since</span>
                }
                <span role="columnheader" class="visually-hidden">Actions</span>
              </div>
              <cdk-virtual-scroll-viewport [itemSize]="rowHeight" class="viewport" [style.height.px]="viewportHeight()">
                <div *cdkVirtualFor="let r of rows(); trackBy: rowKey; let i = index" class="row" role="row" [class.group-row]="r.kind === 'group'" [class.selected]="selectedKey() === r.key" [attr.aria-rowindex]="i + 1" [attr.aria-expanded]="r.kind === 'group' ? r.open : null" tabindex="-1" (contextmenu)="pick(r); r.kind === 'item' ? openContextMenu($event, rowMenuAnchor, rowMenuTrigger, { row: r }) : null" (keydown.shift.f10)="pick(r)" (click)="pick(r)" (keyup.space)="pick(r)" (dblclick)="activate(r)">
                  @if (r.kind === 'group') {
                    <span class="gname" role="gridcell">
                      <button type="button" matIconButton class="chev" [attr.aria-label]="(r.open ? 'Collapse ' : 'Expand ') + r.title" (click)="toggle(r.key, $event)"><dy-icon [name]="r.open ? 'chevron-down' : 'chevron-right'" /></button>
                      @if (group() === 'process') { <span class="av" aria-hidden="true">{{ ini(r.title) }}</span> }
                      <strong class="truncate">{{ r.title }}</strong>
                      <span class="meta">{{ r.count }} {{ tab() === 'connections' ? (r.count === 1 ? 'connection' : 'connections') : (r.count === 1 ? 'port' : 'ports') }}@if (tab() === 'connections' && group() === 'process') { · {{ r.hosts }} {{ r.hosts === 1 ? 'host' : 'hosts' }} }@if (r.exposed) { · <span class="exp">{{ r.exposed }} exposed</span> }</span>
                      @if (tab() === 'connections' && group() === 'process' && trafficOf(r.title); as tr) { <span class="traffic num" matTooltip="Bytes per second over the last few seconds">↓ {{ rate(tr.rxBps) }} · ↑ {{ rate(tr.txBps) }}</span> }
                    </span>
                  } @else if (tab() === 'connections') {
                    @let c = conn(r);
                    @if (group() !== 'process') {
                      <span class="cell prog" role="gridcell"><span class="av" aria-hidden="true">{{ ini(name(c.owner)) }}</span><span class="truncate" [matTooltip]="reason(c.owner) ?? ''">{{ name(c.owner) }}</span></span>
                    }
                    <span class="cell remote" role="gridcell" [class.indent]="group() === 'process'"><span class="truncate" [matTooltip]="c.remote">{{ short(c.remote) }}</span><span class="addr">:{{ c.remotePort }}</span></span>
                    @if (store.resolveNames()) {
                      <span class="cell muted" role="gridcell"><span class="truncate" [matTooltip]="hostName(c) ?? ''">{{ hostName(c) ?? (c.lookupOk ? '—' : 'Private address') }}</span></span>
                    }
                    <span class="cell" role="gridcell">{{ service(c.remotePort) }}</span>
                    <span class="cell muted" role="gridcell">{{ protoText(c.proto, c.ipv6) }}</span>
                    <span class="cell" role="gridcell"><span class="dot" [attr.data-tone]="state(c).tone" aria-hidden="true"></span>{{ state(c).text }}</span>
                    @if (showLocal()) { <span class="cell addr" role="gridcell">{{ ep(c.local, c.localPort) }}</span> }
                    <span class="cell muted" role="gridcell">{{ since(c.firstSeen) }}</span>
                  } @else {
                    @let l = lis(r);
                    @if (group() !== 'process') {
                      <span class="cell prog" role="gridcell"><span class="av" aria-hidden="true">{{ ini(name(l.owner)) }}</span><span class="truncate" [matTooltip]="reason(l.owner) ?? ''">{{ name(l.owner) }}</span></span>
                    }
                    <span class="cell" role="gridcell" [class.indent]="group() === 'process'">{{ service(l.port) }}</span>
                    <span class="cell" role="gridcell"><span class="addr">{{ l.local }}</span> <span class="reach" [attr.data-tone]="reachTone(l)" [matTooltip]="badge(l).hint"><dy-icon [name]="l.exposure === 'allInterfaces' ? 'globe' : l.exposure === 'loopback' ? 'lock' : 'network'" /> {{ badge(l).label }}</span></span>
                    <span class="cell muted" role="gridcell">{{ protoText(l.proto, l.ipv6) }}</span>
                    <span class="cell muted" role="gridcell">{{ since(l.firstSeen) }}</span>
                  }
                  @if (r.kind === 'item') {
                  <span class="cell acts" role="gridcell">
                    @if (r.kind === 'item') {
                      @if (tab() === 'connections') {
                        @let c2 = conn(r);
                        <button type="button" matIconButton [disabled]="!!blocked(c2)" matTooltipPosition="left" [matTooltip]="'Look up IP ' + c2.remote" [attr.aria-label]="'Look up IP ' + c2.remote" (click)="lookup(c2, $event)"><dy-icon name="external" /></button>
                        <button type="button" matIconButton matTooltipPosition="left" [matTooltip]="'Copy connection to ' + c2.remote" [attr.aria-label]="'Copy connection to ' + c2.remote" (click)="copyRow(r, $event)"><dy-icon name="copy" /></button>
                      } @else {
                        <button type="button" matIconButton matTooltipPosition="left" [matTooltip]="'Copy listener'" aria-label="Copy listener" (click)="copyRow(r, $event)"><dy-icon name="copy" /></button>
                      }
                      <a matIconButton routerLink="/processes" matTooltipPosition="left" [matTooltip]="'Show process'" aria-label="Show process" (click)="$event.stopPropagation()"><dy-icon name="processes" /></a>
                    }
                  </span>
                  }
                </div>
              </cdk-virtual-scroll-viewport>
              @if (rows().length === 0) {
                <p class="empty">{{ tab() === 'connections' ? 'No connections with these filters.' : 'Nothing is listening with these filters.' }}</p>
              }
            </div>

            @if (selectedItem(); as sel) {
              <aside class="detail" aria-label="Details">
                <div class="dhead">
                  <strong>{{ detailTitle(sel) }}</strong>
                  <button type="button" matIconButton aria-label="Close details" (click)="selectedKey.set(null)"><dy-icon name="close" /></button>
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
    <span #rowMenuAnchor class="dy-ctx-anchor" aria-hidden="true" #rowMenuTrigger="matMenuTrigger" [matMenuTriggerFor]="rowMenu"></span>
    <mat-menu #rowMenu="matMenu" aria-label="Row actions">
      <ng-template matMenuContent let-row="row">
        @if (row.kind === 'item') {
          @if (isConn(row.item)) {
            <button mat-menu-item [disabled]="!!blocked(asConn(row.item))" (click)="lookup(asConn(row.item))"><dy-icon name="external" /> Look up IP</button>
            <button mat-menu-item [disabled]="!asConn(row.item).lookupOk" (click)="store.resolveOne(asConn(row.item).remote)"><dy-icon name="globe" /> Resolve host name</button>
            <button mat-menu-item (click)="copyText(asConn(row.item).remote)"><dy-icon name="copy" /> Copy address</button>
            <button mat-menu-item (click)="store.setQuery({ search: asConn(row.item).remote })"><dy-icon name="filter" /> Filter to this host</button>
          } @else {
            <button mat-menu-item (click)="copyText(listenerLine(asListener(row.item)))"><dy-icon name="copy" /> Copy listener</button>
          }
          <button mat-menu-item (click)="copyRow(row)"><dy-icon name="copy" /> Copy as text</button>
          <button mat-menu-item (click)="showProcess()"><dy-icon name="processes" /> Show process</button>
          @if (row.item.owner.kind === 'process') {
            <button mat-menu-item (click)="searchWeb(row.item.owner.name)"><dy-icon name="search" /> Search the web…</button>
            @if (row.item.owner.path) {
              <button mat-menu-item (click)="searchHash(row.item.owner.path, row.item.owner.name)"><dy-icon name="shield" /> Search file hash on VirusTotal…</button>
            }
          }
        }
      </ng-template>
    </mat-menu>
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
    .grid { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); overflow: hidden; outline-offset: 2px; }
    .head, .row { display: grid; grid-template-columns: var(--cols); align-items: center; gap: var(--sp-3); padding: 0 var(--sp-3); }
    .head { height: 30px; border-bottom: 1px solid var(--border); background: var(--surface-1); color: var(--text-3); font-size: var(--fs-xs); position: sticky; top: 0; z-index: 3; }
    .row { height: 36px; border-bottom: 1px solid var(--border-subtle); font-size: var(--fs-sm); cursor: default; }
    .row.group-row { background: var(--surface-inset); }
    .row.group-row .gname { grid-column: 1 / -1; }
    .row:hover { background: var(--surface-hover); }
    .row.selected { background: var(--accent-soft); box-shadow: inset 2px 0 0 var(--accent); }
    .cell { min-width: 0; display: flex; align-items: center; gap: var(--sp-2); overflow: hidden; white-space: nowrap; }
    .cell.indent { padding-left: 26px; }
    .muted { color: var(--text-2); }
    .truncate { overflow: hidden; text-overflow: ellipsis; }
    .addr { font: var(--fw-regular) var(--fs-xs) / 1 var(--font-mono); color: var(--text-3); }
    .gname { display: flex; align-items: center; gap: var(--sp-2); min-width: 0; }
    .meta { color: var(--text-3); font-size: var(--fs-xs); white-space: nowrap; }
    .exp { color: var(--warn); }
    .av { flex: none; display: grid; place-items: center; width: 20px; height: 20px; border-radius: 5px; background: var(--surface-active); color: var(--text-2); font: var(--fw-semibold) 11px / 1 var(--font-sans); }
    .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--border-strong); flex: none; }
    .dot[data-tone='ok'] { background: var(--ok); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .acts { justify-content: flex-end; gap: 2px; opacity: 0; transition: opacity var(--dur-fast) var(--ease-out); }
    .row:hover .acts, .row:focus-within .acts, .row.selected .acts { opacity: 1; }
    .empty { margin: 0; padding: var(--sp-6); text-align: center; color: var(--text-3); }
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
  protected readonly openContextMenu = openContextMenu;
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
  protected readonly trafficByName = computed(() => new Map((this.store.snapshot()?.traffic ?? []).map((t) => [t.name, t])));
  protected readonly trafficOf = (title: string) => this.trafficByName().get(title) ?? null;
  protected readonly rate = formatRate;
  protected readonly hostName = (c: ConnectionRow): string | null => this.store.names()[c.remote] ?? null;
  protected readonly noteDismissed = signal(false);
  protected readonly short = (a: string) => middleEllipsis(a);
  protected readonly rowHeight = ROW_H;
  private readonly viewport = viewChild(CdkVirtualScrollViewport);

  protected readonly connections = computed<ConnectionRow[]>(() => this.store.snapshot()?.connections ?? []);
  protected readonly listeners = computed<ListenerRow[]>(() => sortListeners(this.store.snapshot()?.listeners ?? []));
  protected readonly sum = computed(() => summarise(this.connections(), this.listeners()));
  protected readonly rows = computed<Row[]>(() => {
    const by = this.tab() === 'listening' && this.group() === 'host' ? 'none' : this.group();
    return this.tab() === 'connections'
      ? tableRows(this.connections(), by, this.collapsed())
      : tableRows(this.listeners(), by, this.collapsed(), (l) => l.exposure === 'allInterfaces');
  });
  protected readonly viewportHeight = computed(() => Math.min(Math.max(this.rows().length, 1) * ROW_H, 560));
  protected readonly cols = computed(() => {
    const prog = this.group() !== 'process' ? 'minmax(130px, 1.2fr) ' : '';
    return this.tab() === 'connections'
      ? `${prog}minmax(160px, 2fr) ${this.store.resolveNames() ? 'minmax(140px, 2fr) ' : ''}minmax(90px, 1fr) 80px 110px ${this.showLocal() ? 'minmax(150px, 1.2fr) ' : ''}80px 84px`
      : `${prog}minmax(240px, 2fr) minmax(220px, 2fr) 80px 90px 84px`;
  });
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
  protected readonly rowKey = (_: number, r: Row) => r.key;
  protected readonly conn = (r: Row) => (r as { item: ConnectionRow }).item;
  protected readonly lis = (r: Row) => (r as { item: ListenerRow }).item;
  protected readonly isConn = (x: ConnectionRow | ListenerRow): x is ConnectionRow => 'remote' in x;
  protected readonly asConn = (x: ConnectionRow | ListenerRow) => x as ConnectionRow;
  protected readonly asListener = (x: ConnectionRow | ListenerRow) => x as ListenerRow;
  private searchTimer: ReturnType<typeof setTimeout> | null = null;

  ngOnInit(): void {
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

  protected pick(r: Row): void {
    this.selectedKey.set(r.key);
  }

  protected activate(r: Row): void {
    if (r.kind === 'group') this.toggle(r.key);
  }

  protected key(e: KeyboardEvent): void {
    const rows = this.rows();
    if (rows.length === 0) return;
    const i = Math.max(0, rows.findIndex((r) => r.key === this.selectedKey()));
    const go = (n: number) => {
      const j = Math.min(rows.length - 1, Math.max(0, n));
      this.selectedKey.set(rows[j]!.key);
      this.viewport()?.scrollToIndex(Math.max(0, j - 2));
    };
    const cur = rows[i]!;
    const mod = e.metaKey || e.ctrlKey;
    if (e.key === 'ArrowDown') go(this.selectedKey() === null ? 0 : i + 1);
    else if (e.key === 'ArrowUp') go(i - 1);
    else if (e.key === 'Home') go(0);
    else if (e.key === 'End') go(rows.length - 1);
    else if (e.key === 'ArrowLeft' && cur.kind === 'group' && cur.open) this.toggle(cur.key);
    else if (e.key === 'ArrowRight' && cur.kind === 'group' && !cur.open) this.toggle(cur.key);
    else if (e.key === 'Enter') {
      if (cur.kind === 'group') this.toggle(cur.key);
      else this.selectedKey.set(cur.key);
    } else if (e.key === 'Escape') this.selectedKey.set(null);
    else if (mod && e.key.toLowerCase() === 'c' && cur.kind === 'item' && !window.getSelection()?.toString()) void this.copyRow(cur);
    else return;
    e.preventDefault();
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
