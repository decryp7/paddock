import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { ConnectionRow, ListenerRow, NetProto } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { NetworkStore } from '../../state/network.store';
import { endpoint, exposureBadge, groupConnections, listenerLine, lookupBlockedReason, ownerName, ownerReason } from './network-model';

type Tab = 'listening' | 'connections';
type Group = 'none' | 'process' | 'host';

/**
 * Open ports and connections per program. Observe-only: Dyno lists, it never closes a connection. Loopback is hidden by
 * default; an "Exposed" badge marks listeners reachable from the network; "Look up IP" opens the user's lookup page in
 * the browser for public peers.
 */
@Component({
  selector: 'dy-network-page',
  imports: [AsyncState, Icon, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Network</h1>
          <p>Which programs listen for connections and who they talk to. Dyno only looks: it never closes a connection or changes a firewall.</p>
        </div>
      </header>

      <div class="bar" role="group" aria-label="Filters">
        <div class="seg" role="group" aria-label="Show">
          <button type="button" [attr.aria-pressed]="tab() === 'listening'" (click)="tab.set('listening')">Listening ({{ listeners().length }})</button>
          <button type="button" [attr.aria-pressed]="tab() === 'connections'" (click)="tab.set('connections')">Connections ({{ connections().length }})</button>
        </div>
        <label class="search"><dy-icon name="search" /><input class="input" type="search" placeholder="Port, address or program" aria-label="Search the network table" [value]="store.query().search" (input)="search($any($event.target).value)" /></label>
        <label class="chk"><input type="checkbox" [checked]="store.query().showLoopback" (change)="store.setQuery({ showLoopback: $any($event.target).checked })" /> Show loopback</label>
        <label class="chk"><input type="checkbox" [checked]="store.query().exposedOnly" (change)="store.setQuery({ exposedOnly: $any($event.target).checked })" /> Exposed only</label>
        <select class="select sm" aria-label="Protocol" (change)="proto($any($event.target).value)">
          <option value="">TCP and UDP</option>
          <option value="tcp">TCP</option>
          <option value="udp">UDP</option>
        </select>
        @if (tab() === 'connections') {
          <select class="select sm" aria-label="Group by" (change)="group.set($any($event.target).value)">
            <option value="none">No grouping</option>
            <option value="process">Group by program</option>
            <option value="host">Group by remote host</option>
          </select>
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

          @if (tab() === 'listening') {
            <section class="card" aria-label="Listening ports">
              <table class="table">
                <caption class="visually-hidden">Listening ports</caption>
                <thead><tr><th>Program</th><th>Protocol</th><th>Address</th><th class="r">Port</th><th>Reach</th><th>Since</th><th><span class="visually-hidden">Actions</span></th></tr></thead>
                <tbody>
                  @for (l of listeners(); track $index) {
                    <tr>
                      <td><strong [title]="reason(l.owner) ?? path(l)">{{ name(l.owner) }}</strong>@if (pid(l); as p) { <span class="faint num"> · {{ p }}</span> }</td>
                      <td>{{ l.proto === 'tcp' ? 'TCP' : 'UDP' }} {{ l.ipv6 ? 'IPv6' : 'IPv4' }}</td>
                      <td class="mono">{{ l.local }}</td>
                      <td class="r num">{{ l.port }}</td>
                      <td><span class="badge" [attr.data-tone]="badge(l).tone" [title]="badge(l).hint">{{ badge(l).label }}</span></td>
                      <td>{{ since(l.firstSeen) }}</td>
                      <td class="r actions">
                        <a class="btn sm ghost" routerLink="/processes" title="Show the program in the task manager"><dy-icon name="processes" /> Show process</a>
                        <button type="button" class="btn sm ghost" [attr.aria-label]="'Copy ' + line(l)" (click)="copy(line(l))"><dy-icon name="copy" /></button>
                      </td>
                    </tr>
                  } @empty {
                    <tr><td colspan="7" class="faint">Nothing is listening with these filters.</td></tr>
                  }
                </tbody>
              </table>
            </section>
          } @else {
            @for (g of groups(); track g.key) {
              <section class="card" [attr.aria-label]="g.title || 'Connections'">
                @if (g.title) {
                  <div class="card-head"><h2>{{ g.title }}</h2><span class="spacer"></span><span class="badge">{{ g.rows.length }}</span></div>
                }
                <table class="table">
                  <caption class="visually-hidden">Connections</caption>
                  <thead><tr><th>Program</th><th>Local</th><th>Remote</th><th>State</th><th>Since</th><th><span class="visually-hidden">Actions</span></th></tr></thead>
                  <tbody>
                    @for (c of g.rows; track $index) {
                      <tr>
                        <td><strong [title]="reason(c.owner) ?? ''">{{ name(c.owner) }}</strong></td>
                        <td class="mono">{{ ep(c.local, c.localPort) }}</td>
                        <td class="mono">{{ ep(c.remote, c.remotePort) }}</td>
                        <td><span class="badge" [attr.data-tone]="c.state === 'established' ? 'ok' : null">{{ c.stateLabel }}</span></td>
                        <td>{{ since(c.firstSeen) }}</td>
                        <td class="r actions">
                          <button type="button" class="btn sm ghost" [disabled]="!!blocked(c)" [attr.title]="blocked(c) ?? 'Open the lookup page for this address in your browser'" [attr.aria-label]="'Look up IP ' + c.remote" (click)="store.lookup(c.remote)"><dy-icon name="external" /> Look up IP</button>
                          <button type="button" class="btn sm ghost" [attr.aria-label]="'Copy ' + ep(c.remote, c.remotePort)" (click)="copy(ep(c.remote, c.remotePort))"><dy-icon name="copy" /></button>
                        </td>
                      </tr>
                    }
                  </tbody>
                </table>
              </section>
            } @empty {
              <section class="card"><p class="note">No connections with these filters.</p></section>
            }
          }
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    dy-async { display: grid; gap: var(--sp-4); }
    .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; }
    .search { position: relative; display: block; min-width: 240px; }
    .search dy-icon { position: absolute; left: 8px; top: 7px; --icon-size: 15px; color: var(--text-3); }
    .search input { padding-left: 30px; width: 100%; }
    .chk { display: inline-flex; gap: var(--sp-2); align-items: center; font-size: var(--fs-sm); color: var(--text-2); }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); padding: var(--sp-2) var(--sp-3); }
    .card { padding-bottom: var(--sp-2); }
    .table { margin: 0 var(--sp-4) var(--sp-2); width: calc(100% - 2 * var(--sp-4)); table-layout: fixed; }
    .table td { overflow: hidden; text-overflow: ellipsis; }
    .table td.actions { overflow: visible; text-overflow: clip; white-space: nowrap; width: 230px; }
    .table th:last-child { width: 230px; }
    .faint { color: var(--text-3); }
    .mono { font-family: var(--font-mono); font-size: var(--fs-sm); }
  `,
})
export class NetworkPage implements OnInit, OnDestroy {
  protected readonly store = inject(NetworkStore);
  private readonly notifier = inject(Notifier);
  protected readonly tab = signal<Tab>('listening');
  protected readonly group = signal<Group>('process');
  protected readonly listeners = computed<ListenerRow[]>(() => this.store.snapshot()?.listeners ?? []);
  protected readonly connections = computed<ConnectionRow[]>(() => this.store.snapshot()?.connections ?? []);
  protected readonly groups = computed(() => groupConnections(this.connections(), this.group()));
  protected readonly name = ownerName;
  protected readonly reason = ownerReason;
  protected readonly badge = (l: ListenerRow) => exposureBadge(l.exposure);
  protected readonly blocked = lookupBlockedReason;
  protected readonly ep = endpoint;
  protected readonly line = listenerLine;
  private searchTimer: ReturnType<typeof setTimeout> | null = null;

  ngOnInit(): void {
    void this.store.start();
  }

  ngOnDestroy(): void {
    this.store.stop();
    if (this.searchTimer !== null) clearTimeout(this.searchTimer);
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

  protected pid(l: ListenerRow): number | null {
    return l.owner.kind === 'process' ? l.owner.pid : null;
  }

  protected path(l: ListenerRow): string {
    return l.owner.kind === 'process' ? (l.owner.path ?? '') : '';
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
