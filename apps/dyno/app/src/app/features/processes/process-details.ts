import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { ListOrReason, ProcessDetails } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { formatAffinity, formatBytes, formatCount, formatPercent, formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { PRIORITY_LABEL, signatureText } from '../../shared/labels';
import { LiveStore } from '../../state/live.store';
import { ProcessesStore } from '../../state/processes.store';

type Tab = 'overview' | 'parents' | 'files' | 'connections' | 'modules';

/** The side panel of one process: facts, parent chain and the lists the OS may or may not let Dyno read (with the reason when not). */
@Component({
  selector: 'dy-process-details',
  imports: [Icon, NgTemplateOutlet],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <aside class="panel" aria-label="Process details">
      <header>
        <div class="titles">
          <h2 class="truncate">{{ d()?.process?.name ?? 'Process' }}</h2>
          @if (d(); as x) {
            <span class="faint num">PID {{ pidText(x.process.pid) }} · {{ x.process.user ?? 'unknown user' }}</span>
          }
        </div>
        <button type="button" class="btn ghost icon sm" aria-label="Close details" (click)="close()"><dy-icon name="close" /></button>
      </header>
      @if (store.detailsError(); as e) {
        <div class="err" role="alert">
          <dy-icon name="alert" />
          <div class="text"><strong>{{ e.message }}</strong>@if (hint(); as h) {<span>{{ h }}</span>}</div>
          <button type="button" class="btn sm" (click)="reload()">Retry</button>
        </div>
      } @else if (store.detailsLoading() || !d()) {
        <div class="skeletons" aria-busy="true"><div class="skeleton"></div><div class="skeleton" style="width: 70%"></div><div class="skeleton"></div></div>
      } @else {
        @if (d(); as x) {
          @if (x.process.protectedReason; as why) {
            <p class="lock"><dy-icon name="lock" /> {{ why }}. Dyno never changes it.</p>
          }
          <div class="tabs" role="tablist" aria-label="Details">
            @for (t of tabs; track t.id) {
              <button type="button" role="tab" [attr.aria-selected]="tab() === t.id" (click)="tab.set(t.id)">{{ t.label }}</button>
            }
          </div>
          <div class="content" role="tabpanel">
            @switch (tab()) {
              @case ('overview') {
                <dl class="dy-selectable">
                  <dt>Command line</dt><dd class="mono wrap">{{ x.commandLine.join(' ') || 'Not available' }}</dd>
                  <dt>Folder</dt><dd class="mono wrap">{{ x.cwd ?? 'Not available' }}</dd>
                  <dt>Signed by</dt>
                  <dd>
                    <span class="badge" [attr.data-tone]="sig().tone">@if (sig().verified) {<dy-icon name="shield-check" />} {{ sig().text }}</span>
                  </dd>
                  <dt>Started</dt><dd class="num">{{ since(x.process.started) }}</dd>
                  <dt>Priority</dt><dd>{{ x.priority ? priorityLabel(x.priority) : 'Not available on this PC' }}</dd>
                  <dt>Cores</dt><dd class="num">{{ x.affinity === null ? 'Not available on this PC' : affinity(x.affinity) }}</dd>
                  <dt>Efficiency mode</dt><dd>{{ x.efficiencyMode === null ? 'Not available on this PC' : x.efficiencyMode ? 'On' : 'Off' }}</dd>
                  <dt>Threads</dt><dd class="num">{{ x.threads === null ? '—' : count(x.threads) }}</dd>
                  <dt>Handles</dt><dd class="num">{{ x.handles === null ? '—' : count(x.handles) }}</dd>
                  <dt>Starts with</dt><dd>{{ x.startupSource ?? 'Not a startup item' }}</dd>
                  <dt>Now</dt><dd class="num">{{ pct(x.process.cpu) }} CPU · {{ bytes(x.process.memory) }}</dd>
                </dl>
              }
              @case ('parents') {
                @if (x.parents.length === 0) {
                  <p class="faint">This process has no parent (it is a root).</p>
                }
                <ol class="chain">
                  <li class="me"><dy-icon name="processes" /> {{ x.process.name }}</li>
                  @for (p of x.parents; track p.pid) {
                    <li><button type="button" class="btn link" (click)="open(p.pid)">{{ p.name }}</button> <span class="faint num">PID {{ pidText(p.pid) }}</span></li>
                  }
                </ol>
              }
              @case ('files') {
                <ng-container *ngTemplateOutlet="list; context: { l: x.openFiles, empty: 'No open files.' }" />
              }
              @case ('connections') {
                @if (conns(x.connections); as c) {
                  @if (c.reason) {
                    <p class="na">{{ c.reason }}</p>
                  } @else if (c.items.length === 0) {
                    <p class="faint">No network connections.</p>
                  } @else {
                    <ul class="plain dy-selectable">
                      @for (n of c.items; track $index) {
                        <li class="mono"><b>{{ n.protocol }}</b> {{ n.local }} → {{ n.remote }} <span class="faint">{{ n.state }}</span></li>
                      }
                    </ul>
                  }
                }
              }
              @case ('modules') {
                <ng-container *ngTemplateOutlet="list; context: { l: x.modules, empty: 'No loaded libraries.' }" />
              }
            }
          </div>
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
    .tabs { display: flex; gap: 2px; padding: 0 var(--sp-3); border-bottom: 1px solid var(--border); }
    .tabs button { height: 32px; padding: 0 var(--sp-3); border: 0; border-bottom: 2px solid transparent; background: none; color: var(--text-2); font: var(--fw-medium) var(--fs-sm) var(--font-sans); cursor: default; }
    .tabs button[aria-selected='true'] { color: var(--text-1); border-bottom-color: var(--accent); }
    .content { flex: 1; overflow: auto; padding: var(--sp-3) var(--sp-4) var(--sp-4); }
    dl { margin: 0; display: grid; gap: 2px var(--sp-3); grid-template-columns: 96px minmax(0, 1fr); font-size: var(--fs-sm); }
    dt { color: var(--text-3); padding: 3px 0; }
    dd { margin: 0; padding: 3px 0; }
    .wrap { word-break: break-all; font-size: var(--fs-xs); }
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
    .skeletons { display: grid; gap: var(--sp-3); padding: var(--sp-4); }
  `,
})
export class ProcessDetailsPanel {
  protected readonly store = inject(ProcessesStore);
  private readonly live = inject(LiveStore);
  protected readonly tab = signal<Tab>('overview');
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
  protected readonly since = (at: number) => formatRelative(at, this.live.system()?.at ?? Date.now());
  protected strings(l: ListOrReason<string>) {
    return 'unavailable' in l ? { reason: l.unavailable, items: [] as string[], truncated: false } : { reason: null, items: l.items.items, truncated: l.items.truncated };
  }
  protected conns(l: ListOrReason<import('../../core/api.types').Connection>) {
    return 'unavailable' in l ? { reason: l.unavailable, items: [] } : { reason: null, items: l.items.items };
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
    if (p) void this.store.select({ pid: p.pid, started: p.started });
  }
}
