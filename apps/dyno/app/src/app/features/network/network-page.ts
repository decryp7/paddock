import { CdkFixedSizeVirtualScroll, CdkVirtualForOf, CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, signal, viewChild } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { ConnectionRow, ListenerRow, NetProto } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { NetworkStore } from '../../state/network.store';
import {
  type GroupBy,
  type TableRow,
  connectionLine,
  endpoint,
  exposureBadge,
  initial,
  listenerLine,
  lookupBlockedReason,
  ownerName,
  ownerReason,
  portService,
  protoText,
  sortListeners,
  stateView,
  summarise,
  tableRows,
} from './network-model';

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
  imports: [AsyncState, Icon, RouterLink, CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf],
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
        <div class="summary" role="group" aria-label="Summary">
          <button type="button" class="tile" (click)="showConnections()"><b class="num">{{ sum().connections }}</b> connections</button>
          <span class="tile static"><b class="num">{{ sum().programs }}</b> programs</span>
          <span class="tile static"><b class="num">{{ sum().hosts }}</b> remote hosts</span>
          <button type="button" class="tile" [attr.data-tone]="sum().exposed ? 'warn' : null" (click)="showExposed()"><b class="num">{{ sum().exposed }}</b> exposed listeners</button>
        </div>
      }

      <div class="bar" role="group" aria-label="Filters">
        <div class="seg" role="group" aria-label="Show">
          <button type="button" [attr.aria-pressed]="tab() === 'connections'" (click)="setTab('connections')">Connections</button>
          <button type="button" [attr.aria-pressed]="tab() === 'listening'" (click)="setTab('listening')">Listening</button>
        </div>
        <label class="search"><dy-icon name="search" /><input class="input" type="search" placeholder="Port, address or program" aria-label="Search the network table" [value]="store.query().search" (input)="search($any($event.target).value)" /></label>
        <label class="chk"><input type="checkbox" [checked]="store.query().showLoopback" (change)="store.setQuery({ showLoopback: $any($event.target).checked })" /> Show loopback</label>
        <label class="chk"><input type="checkbox" [checked]="store.query().exposedOnly" (change)="store.setQuery({ exposedOnly: $any($event.target).checked })" /> Exposed only</label>
        <select class="select sm" aria-label="Protocol" (change)="proto($any($event.target).value)">
          <option value="">TCP and UDP</option>
          <option value="tcp">TCP</option>
          <option value="udp">UDP</option>
        </select>
        <select class="select sm" aria-label="Group by" [value]="group()" (change)="setGroup($any($event.target).value)">
          <option value="process">Group by program</option>
          @if (tab() === 'connections') {
            <option value="host">Group by remote host</option>
          }
          <option value="none">No grouping</option>
        </select>
        @if (tab() === 'connections') {
          <label class="chk"><input type="checkbox" [checked]="showLocal()" (change)="showLocal.set($any($event.target).checked)" /> Local address</label>
        }
      </div>

      <dy-async [loaded]="!!store.snapshot()" [error]="store.error()" what="network" [rows]="5" (retry)="store.load()">
        @if (store.snapshot(); as s) {
          @if (s.unknownOwners > 0) {
            <p class="note"><dy-icon name="info" /> {{ s.unknownOwners }} {{ s.unknownOwners === 1 ? 'socket belongs' : 'sockets belong' }} to programs the OS will not name without elevated rights (other users or the system); they show as “Owner unknown”. Dyno never asks for administrator rights for this page.</p>
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
                <div *cdkVirtualFor="let r of rows(); trackBy: rowKey; let i = index" class="row" role="row" [class.group-row]="r.kind === 'group'" [class.selected]="selectedKey() === r.key" [attr.aria-rowindex]="i + 1" [attr.aria-expanded]="r.kind === 'group' ? r.open : null" tabindex="-1" (click)="pick(r)" (keyup.space)="pick(r)" (dblclick)="activate(r)">
                  @if (r.kind === 'group') {
                    <span class="gname" role="gridcell">
                      <button type="button" class="chev" [attr.aria-label]="(r.open ? 'Collapse ' : 'Expand ') + r.title" (click)="toggle(r.key, $event)"><dy-icon [name]="r.open ? 'chevron-down' : 'chevron-right'" /></button>
                      @if (group() === 'process') { <span class="av" aria-hidden="true">{{ ini(r.title) }}</span> }
                      <strong class="truncate">{{ r.title }}</strong>
                      <span class="meta">{{ r.count }} {{ tab() === 'connections' ? (r.count === 1 ? 'connection' : 'connections') : (r.count === 1 ? 'port' : 'ports') }}@if (tab() === 'connections' && group() === 'process') { · {{ r.hosts }} {{ r.hosts === 1 ? 'host' : 'hosts' }} }@if (r.exposed) { · <span class="exp">{{ r.exposed }} exposed</span> }</span>
                    </span>
                  } @else if (tab() === 'connections') {
                    @let c = conn(r);
                    @if (group() !== 'process') {
                      <span class="cell prog" role="gridcell"><span class="av" aria-hidden="true">{{ ini(name(c.owner)) }}</span><span class="truncate" [title]="reason(c.owner) ?? ''">{{ name(c.owner) }}</span></span>
                    }
                    <span class="cell remote" role="gridcell" [class.indent]="group() === 'process'"><span class="truncate" [title]="c.remote">{{ c.remote }}</span><span class="addr">:{{ c.remotePort }}</span></span>
                    <span class="cell" role="gridcell">{{ service(c.remotePort) }}</span>
                    <span class="cell muted" role="gridcell">{{ protoText(c.proto, c.ipv6) }}</span>
                    <span class="cell" role="gridcell"><span class="dot" [attr.data-tone]="state(c).tone" aria-hidden="true"></span>{{ state(c).text }}</span>
                    @if (showLocal()) { <span class="cell addr" role="gridcell">{{ ep(c.local, c.localPort) }}</span> }
                    <span class="cell muted" role="gridcell">{{ since(c.firstSeen) }}</span>
                  } @else {
                    @let l = lis(r);
                    @if (group() !== 'process') {
                      <span class="cell prog" role="gridcell"><span class="av" aria-hidden="true">{{ ini(name(l.owner)) }}</span><span class="truncate" [title]="reason(l.owner) ?? ''">{{ name(l.owner) }}</span></span>
                    }
                    <span class="cell" role="gridcell" [class.indent]="group() === 'process'">{{ service(l.port) }}</span>
                    <span class="cell" role="gridcell"><span class="addr">{{ l.local }}</span> <span class="badge" [attr.data-tone]="badge(l).tone" [title]="badge(l).hint">{{ badge(l).label }}</span></span>
                    <span class="cell muted" role="gridcell">{{ protoText(l.proto, l.ipv6) }}</span>
                    <span class="cell muted" role="gridcell">{{ since(l.firstSeen) }}</span>
                  }
                  @if (r.kind === 'item') {
                  <span class="cell acts" role="gridcell">
                    @if (r.kind === 'item') {
                      @if (tab() === 'connections') {
                        @let c2 = conn(r);
                        <button type="button" class="ib" [disabled]="!!blocked(c2)" [attr.title]="blocked(c2) ?? 'Look up this address in your browser'" [attr.aria-label]="'Look up IP ' + c2.remote" (click)="lookup(c2, $event)"><dy-icon name="external" /></button>
                        <button type="button" class="ib" title="Copy the connection" [attr.aria-label]="'Copy connection to ' + c2.remote" (click)="copyRow(r, $event)"><dy-icon name="copy" /></button>
                      } @else {
                        <button type="button" class="ib" title="Copy the listener" aria-label="Copy listener" (click)="copyRow(r, $event)"><dy-icon name="copy" /></button>
                      }
                      <a class="ib" routerLink="/processes" title="Show the program in the task manager" aria-label="Show process" (click)="$event.stopPropagation()"><dy-icon name="processes" /></a>
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
                  <button type="button" class="ib" aria-label="Close details" (click)="selectedKey.set(null)"><dy-icon name="close" /></button>
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
                    <button type="button" class="btn sm" [disabled]="!!blocked(c3)" [attr.title]="blocked(c3) ?? null" (click)="lookup(c3)"><dy-icon name="external" /> Look up IP</button>
                    <button type="button" class="btn sm" (click)="copyText(connectionLine(c3))"><dy-icon name="copy" /> Copy</button>
                  </div>
                  @if (blocked(c3); as why) { <p class="hint">{{ why }}</p> }
                  <p class="hint">Reverse DNS names are not resolved yet.</p>
                } @else {
                  @let l3 = asListener(sel);
                  <p class="hint">{{ badge(l3).hint }}</p>
                  <div class="dacts"><button type="button" class="btn sm" (click)="copyText(listenerLine(l3))"><dy-icon name="copy" /> Copy</button></div>
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
    .page { display: grid; gap: var(--sp-3); align-content: start; }
    dy-async { display: grid; gap: var(--sp-3); }
    .summary { display: flex; gap: var(--sp-2); flex-wrap: wrap; }
    .tile { display: inline-flex; gap: var(--sp-2); align-items: baseline; padding: 6px 12px; border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-2); color: var(--text-2); font: inherit; font-size: var(--fs-sm); }
    button.tile { cursor: pointer; }
    button.tile:hover { background: var(--surface-hover); }
    .tile b { color: var(--text-1); font: var(--fw-semibold) var(--fs-md) / 1 var(--font-sans); }
    .tile[data-tone='warn'] b { color: var(--warn); }
    .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; }
    .search { position: relative; display: block; min-width: 220px; }
    .search dy-icon { position: absolute; left: 8px; top: 7px; --icon-size: 15px; color: var(--text-3); }
    .search input { padding-left: 30px; width: 100%; }
    .chk { display: inline-flex; gap: var(--sp-2); align-items: center; font-size: var(--fs-sm); color: var(--text-2); }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); }
    .layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .layout.with-detail { grid-template-columns: minmax(0, 1fr) 320px; }
    .grid { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); overflow: hidden; outline-offset: 2px; }
    .head, .row { display: grid; grid-template-columns: var(--cols); align-items: center; gap: var(--sp-3); padding: 0 var(--sp-3); }
    .head { height: 30px; border-bottom: 1px solid var(--border); background: var(--surface-1); color: var(--text-3); font-size: var(--fs-xs); letter-spacing: 0.04em; text-transform: uppercase; position: sticky; top: 0; z-index: 1; }
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
    .chev { display: grid; place-items: center; width: 22px; height: 22px; border: 0; background: none; color: var(--text-3); border-radius: var(--r-sm); cursor: pointer; }
    .chev:hover { background: var(--surface-active); color: var(--text-1); }
    .av { flex: none; display: grid; place-items: center; width: 20px; height: 20px; border-radius: 5px; background: var(--surface-active); color: var(--text-2); font: var(--fw-semibold) 11px / 1 var(--font-sans); }
    .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--border-strong); flex: none; }
    .dot[data-tone='ok'] { background: var(--ok); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .acts { justify-content: flex-end; gap: 2px; opacity: 0; transition: opacity var(--dur-fast) var(--ease-out); }
    .row:hover .acts, .row:focus-within .acts, .row.selected .acts { opacity: 1; }
    .ib { display: grid; place-items: center; width: 24px; height: 24px; border: 0; border-radius: var(--r-sm); background: none; color: var(--text-2); cursor: pointer; }
    .ib:hover:not(:disabled) { background: var(--surface-active); color: var(--text-1); }
    .ib:disabled { opacity: 0.35; cursor: default; }
    .ib dy-icon { --icon-size: 15px; }
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
  protected readonly store = inject(NetworkStore);
  private readonly notifier = inject(Notifier);
  protected readonly tab = signal<Tab>('connections');
  protected readonly group = signal<GroupBy>('process');
  protected readonly showLocal = signal(false);
  protected readonly collapsed = signal<ReadonlySet<string>>(readCollapsed());
  protected readonly selectedKey = signal<string | null>(null);
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
      ? `${prog}minmax(160px, 2fr) minmax(90px, 1fr) 80px 110px ${this.showLocal() ? 'minmax(150px, 1.2fr) ' : ''}80px 84px`
      : `${prog}minmax(120px, 1.4fr) minmax(200px, 2fr) 80px 80px 84px`;
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
