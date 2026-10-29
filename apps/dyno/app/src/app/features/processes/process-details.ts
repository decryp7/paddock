import { NgTemplateOutlet } from '@angular/common';
import { RouterLink } from '@angular/router';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { ListOrReason, ProcessDetails } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { readValue, showMeasured } from '../../core/measured';
import { formatAffinity, formatBytes, formatRate, formatCount, formatPercent, formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { PRIORITY_LABEL, signatureText } from '../../shared/labels';
import { LiveStore } from '../../state/live.store';
import { NetworkStore, type ProcessNetwork } from '../../state/network.store';
import { ProcessesStore } from '../../state/processes.store';
import { toCommandError } from '../../core/command-error';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatTabsModule } from '@angular/material/tabs';
import { MatTooltip } from '@angular/material/tooltip';

type Tab = 'overview' | 'parents' | 'files' | 'connections' | 'modules';

/** The side panel of one process: facts, parent chain and the lists the OS may or may not let Dyno read (with the reason when not). */
@Component({
  selector: 'dy-process-details',
  imports: [MatTooltip, MatTabsModule, MatButton, MatIconButton, Icon, NgTemplateOutlet, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <aside class="panel" aria-label="Process details">
      <header>
        <div class="titles">
          <h2 class="truncate">{{ d()?.process?.name ?? 'Process' }}</h2>
          @if (d(); as x) {
            <span class="faint num">PID {{ pidText(x.process.pid) }} · {{ show(x.process.user, text) }}</span>
          }
        </div>
        <button type="button" matIconButton matTooltip="Close details" aria-label="Close details" (click)="close()"><dy-icon name="close" /></button>
      </header>
      @if (store.detailsError(); as e) {
        <div class="err" role="alert">
          <dy-icon name="alert" />
          <div class="text"><strong>{{ e.message }}</strong>@if (hint(); as h) {<span>{{ h }}</span>}</div>
          <button type="button" matButton="outlined" (click)="reload()">Retry</button>
        </div>
      } @else if (store.detailsLoading() || !d()) {
        <div class="skeletons" aria-busy="true"><div class="skeleton"></div><div class="skeleton" style="width: 70%"></div><div class="skeleton"></div></div>
      } @else {
        @if (d(); as x) {
          <nav mat-tab-nav-bar class="tabs" [tabPanel]="panel" aria-label="Details">
            @for (t of tabs; track t.id) {
              <button type="button" mat-tab-link [active]="tab() === t.id" (click)="pickTab(t.id)">{{ t.label }}</button>
            }
          </nav>
          <mat-tab-nav-panel #panel class="content">
            @switch (tab()) {
              @case ('overview') {
                <dl class="dy-selectable">
                  <dt>Command line</dt>
                  <dd>
                    <span class="mono wrap cmd" [class.open]="cmdOpen()" [matTooltip]="x.commandLine.join(' ')">{{ x.commandLine.join(' ') || 'Not available' }}</span>
                    @if (x.commandLine.length) {
                      <span class="cmd-acts">
                        @if (x.commandLine.join(' ').length > 120) { <button type="button" matButton="text" class="dy-link" (click)="cmdOpen.set(!cmdOpen())">{{ cmdOpen() ? 'Show less' : 'Show more' }}</button> }
                        <button type="button" matButton="text" class="dy-link" (click)="copyCommand(x.commandLine)">Copy</button>
                      </span>
                    }
                  </dd>
                  <dt>Folder</dt><dd class="mono wrap">{{ x.cwd ?? 'Not available' }}</dd>
                  <dt>Signed by</dt>
                  <dd>
                    <span class="badge" [attr.data-tone]="sig().tone">@if (sig().verified) {<dy-icon name="shield-check" />} {{ sig().text }}</span>
                  </dd>
                  <dt>Started</dt><dd class="num">{{ show(x.process.started, since) }}</dd>
                  <dt>Priority</dt><dd>{{ x.priority ? priorityLabel(x.priority) : 'Not available on this computer' }}</dd>
                  <dt>Cores</dt><dd class="num">{{ x.affinity === null ? 'Not available on this computer' : affinity(x.affinity) }}</dd>
                  <dt>Efficiency mode</dt><dd>{{ x.efficiencyMode === null ? 'Not available on this computer' : x.efficiencyMode ? 'On' : 'Off' }}</dd>
                  <dt>Threads</dt><dd class="num">{{ x.threads === null ? '—' : count(x.threads) }}</dd>
                  <dt>Handles</dt><dd class="num">{{ x.handles === null ? '—' : count(x.handles) }}</dd>
                  <dt>Starts with</dt><dd>{{ x.startupSource ?? 'Not a startup item' }}</dd>
                  <dt>Now</dt><dd class="num">{{ show(x.process.cpu, pct) }} CPU · {{ show(x.process.memory, bytes) }}</dd>
                </dl>
              }
              @case ('parents') {
                @if (x.parents.length === 0) {
                  <p class="faint">This process has no parent (it is a root).</p>
                }
                <ol class="chain">
                  <li class="me"><dy-icon name="processes" /> {{ x.process.name }}</li>
                  @for (p of x.parents; track p.pid) {
                    <li><button type="button" matButton="text" class="dy-link" (click)="open(p.pid)">{{ p.name }}</button> <span class="faint num">PID {{ pidText(p.pid) }}</span></li>
                  }
                </ol>
              }
              @case ('files') {
                <ng-container *ngTemplateOutlet="list; context: { l: x.openFiles, empty: 'No open files.' }" />
              }
              @case ('connections') {
                @if (netError(); as e) {
                  <p class="na">{{ e }}</p>
                } @else if (net(); as n) {
                  @if (n.traffic; as t) {
                    <p class="traffic num">↓ {{ rate(t.rxBps) }} · ↑ {{ rate(t.txBps) }}</p>
                  } @else if (n.note) {
                    <p class="faint">{{ n.note }}</p>
                  }
                  @if (n.listeners.length === 0 && n.connections.length === 0) {
                    <p class="faint">Nothing is listening or connected.</p>
                  }
                  @if (n.listeners.length) {
                    <h3>Listening</h3>
                    <ul class="plain dy-selectable">
                      @for (l of n.listeners; track $index) {
                        <li class="mono"><b>{{ l.proto }}</b> {{ l.local }}:{{ l.port }} <span class="faint">{{ l.exposure === 'allInterfaces' ? 'exposed' : l.exposure === 'loopback' ? 'this computer only' : 'one interface' }}</span></li>
                      }
                    </ul>
                  }
                  @if (n.connections.length) {
                    <h3>Connections</h3>
                    <ul class="plain dy-selectable">
                      @for (c of n.connections; track $index) {
                        <li class="mono"><b>{{ c.proto }}</b> {{ c.remote }}:{{ c.remotePort }} <span class="faint">{{ c.stateLabel }}</span></li>
                      }
                    </ul>
                  }
                  <a matButton="text" class="dy-link" routerLink="/network">Open the Network page</a>
                } @else {
                  <p class="faint">Reading the network…</p>
                }
              }
              @case ('modules') {
                <ng-container *ngTemplateOutlet="list; context: { l: x.modules, empty: 'No loaded libraries.' }" />
              }
            }
          </mat-tab-nav-panel>
        }
      }
    </aside>
    <ng-template #list let-l="l" let-empty="empty">
      @if (strings(l); as r) {
        @if (r.reason) {
          <p class="na">{{ r.reason }}</p>
        } @else if (r.items.length === 0) {
          <p class="faint">{{ empty }}</p>
        } @else {
          <ul class="plain dy-selectable">
            @for (f of r.items; track $index) {
              <li class="mono">{{ f }}</li>
            }
          </ul>
          @if (r.truncated) {
            <p class="faint more">Only the first entries are shown.</p>
          }
        }
      }
    </ng-template>
  `,
  styles: `
    :host { display: block; min-height: 0; }
    .panel { height: 100%; display: flex; flex-direction: column; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-lg); overflow: hidden; }
    header { display: flex; align-items: flex-start; gap: var(--sp-2); padding: var(--sp-3) var(--sp-3) var(--sp-2) var(--sp-4); }
    .titles { flex: 1; min-width: 0; display: grid; }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
    .lock { display: flex; gap: var(--sp-2); align-items: center; margin: 0 var(--sp-4) var(--sp-2); padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); background: var(--surface-inset); color: var(--text-2); font-size: var(--fs-sm); }
    .tabs { padding: 0 var(--sp-2); }
    /* The five tabs fit the 380 px panel with the wider fonts of Linux: closer padding here, not a bar that paginates and cuts a label. */
    .tabs .mat-mdc-tab-link.mdc-tab { padding-inline: var(--sp-2); }
    .content { flex: 1; overflow: auto; padding: var(--sp-3) var(--sp-4) var(--sp-4); }
    dl { margin: 0; display: grid; gap: 2px var(--sp-3); grid-template-columns: 96px minmax(0, 1fr); font-size: var(--fs-sm); }
    dt { color: var(--text-3); padding: 3px 0; }
    dd { margin: 0; padding: 3px 0; }
    .wrap { word-break: break-all; font-size: var(--fs-sm); }
    .cmd { display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
    .cmd.open { display: block; -webkit-line-clamp: unset; }
    .cmd-acts { display: flex; gap: var(--sp-3); }
    .chain, .plain { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-2); font-size: var(--fs-sm); }
    .chain li::before { content: '↑ '; color: var(--text-3); }
    .chain li.me::before { content: ''; }
    .chain li.me { font-weight: var(--fw-semibold); }
    .plain li { word-break: break-all; font-size: var(--fs-xs); }
    .na { color: var(--text-2); padding: var(--sp-3); background: var(--surface-inset); border-radius: var(--r-md); font-size: var(--fs-sm); }
    .err { display: flex; gap: var(--sp-2); padding: var(--sp-3) var(--sp-4); }
    .err .text { flex: 1; display: grid; }
    .err .text span { color: var(--text-2); font-size: var(--fs-sm); }
    .more { margin-top: var(--sp-2); }
    h3 { margin: var(--sp-3) 0 var(--sp-1); font-size: var(--fs-sm); color: var(--text-3); font-weight: var(--fw-medium); }
    .traffic { margin: 0 0 var(--sp-2); font-size: var(--fs-sm); }
    .skeletons { display: grid; gap: var(--sp-3); padding: var(--sp-4); }
  `,
})
export class ProcessDetailsPanel {
  protected readonly store = inject(ProcessesStore);
  private readonly live = inject(LiveStore);
  protected readonly tab = signal<Tab>('overview');
  protected readonly cmdOpen = signal(false);
  private readonly network = inject(NetworkStore);
  protected readonly net = signal<ProcessNetwork | null>(null);
  protected readonly netError = signal<string | null>(null);
  protected readonly rate = formatRate;
  protected readonly tabs: { id: Tab; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'parents', label: 'Parents' },
    { id: 'files', label: 'Files' },
    { id: 'connections', label: 'Network' },
    { id: 'modules', label: 'Libraries' },
  ];
  protected readonly d = computed<ProcessDetails | null>(() => this.store.details());
  protected readonly hint = computed(() => {
    const e = this.store.detailsError();
    return e ? errorHint(e.code) : null;
  });
  protected readonly sig = computed(() => signatureText(this.d()?.signature ?? { unknown: 'Unknown' }));
  protected readonly pidText = (n: number) => String(n);
  protected readonly pct = formatPercent;
  protected readonly bytes = formatBytes;
  protected readonly count = formatCount;
  protected readonly priorityLabel = (p: keyof typeof PRIORITY_LABEL) => PRIORITY_LABEL[p];
  protected readonly affinity = (mask: number) => formatAffinity(mask, this.live.system()?.cpu.perCore.length ?? 1);
  protected readonly show = showMeasured;
  protected readonly text = (v: string): string => v;
  protected readonly since = (at: number) => formatRelative(at, this.live.system()?.at ?? Date.now());
  protected strings(l: ListOrReason<string>) {
    return 'unavailable' in l ? { reason: l.unavailable, items: [] as string[], truncated: false } : { reason: null, items: l.items.items, truncated: l.items.truncated };
  }

  protected pickTab(id: Tab): void {
    this.tab.set(id);
    if (id === 'connections') void this.loadNetwork();
  }

  private async loadNetwork(): Promise<void> {
    const pid = this.d()?.process.pid;
    if (pid === undefined) return;
    this.net.set(null);
    this.netError.set(null);
    try {
      this.net.set(await this.network.forProcess(pid));
    } catch (e) {
      this.netError.set(toCommandError(e).message);
    }
  }

  protected copyCommand(line: string[]): void {
    void navigator.clipboard?.writeText(line.join(' ')).catch(() => undefined);
  }

  protected close(): void {
    void this.store.select(null);
  }
  protected reload(): void {
    const s = this.store.selected();
    if (s) void this.store.select(s);
  }
  protected open(pid: number): void {
    const p = this.store.processes().find((x) => x.pid === pid);
    if (p) void this.store.select({ pid: p.pid, started: readValue(p.started) });
  }
}
