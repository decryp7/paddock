import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, effect, inject, input } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { firstValueFrom } from 'rxjs';
import type { Priority } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { Dialogs } from '../../shared/dialog';
import { formatBytes, formatCount, formatPercent, formatRate, plural } from '../../shared/format';
import { heat } from '../../shared/heat';
import { Icon } from '../../shared/icon';
import { PRIORITIES, PRIORITY_LABEL } from '../../shared/labels';
import { LiveStore } from '../../state/live.store';
import { ProcessesStore } from '../../state/processes.store';
import { AffinityDialog, CapDialog, LockersDialog } from './process-dialogs';
import { ProcessDetailsPanel } from './process-details';
import { type Row, type SortKey, type ViewMode } from './process-rows';

const COLUMNS: { key: SortKey; label: string; align: 'l' | 'r' }[] = [
  { key: 'name', label: 'Name', align: 'l' },
  { key: 'cpu', label: 'CPU', align: 'r' },
  { key: 'memory', label: 'Memory', align: 'r' },
  { key: 'disk', label: 'Disk', align: 'r' },
  { key: 'user', label: 'User', align: 'l' },
  { key: 'pid', label: 'PID', align: 'r' },
];

/** The task manager: every app and process, grouped by app, as a tree or flat; sortable, searchable, with actions and a details panel. */
@Component({
  selector: 'dy-processes-page',
  imports: [Icon, FormsModule, CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf, CdkMenu, CdkMenuItem, CdkMenuTrigger, CdkContextMenuTrigger, ProcessDetailsPanel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="wrap">
      <header class="head">
        <div class="titles">
          <h1>Processes</h1>
          <p class="num">{{ summary() }}</p>
        </div>
        <div class="seg" role="group" aria-label="View">
          @for (m of modes; track m.id) {
            <button type="button" [attr.aria-pressed]="store.mode() === m.id" (click)="store.setMode(m.id)"><dy-icon [name]="m.icon" /> {{ m.label }}</button>
          }
        </div>
        <label class="search">
          <dy-icon name="search" />
          <input type="search" placeholder="Filter by name, user or PID" aria-label="Filter processes" [ngModel]="store.query()" (ngModelChange)="store.setQuery($event)" autocomplete="off" spellcheck="false" />
        </label>
        <button type="button" class="btn" (click)="lockers()"><dy-icon name="lock" /> What is locking this file?</button>
      </header>

      @if (store.actionError(); as e) {
        <div class="banner" role="alert">
          <dy-icon name="alert" />
          <span><strong>{{ e.message }}</strong> @if (hint(); as h) {<small>{{ h }}</small>}</span>
          <button type="button" class="btn ghost icon sm" aria-label="Dismiss" (click)="store.dismissError()"><dy-icon name="close" /></button>
        </div>
      }

      <div class="body" [class.open]="!!store.selected()">
        <section class="table card" role="treegrid" aria-label="Processes" [attr.aria-rowcount]="store.rows().length + 1">
          <div class="row head-row" role="row">
            @for (c of columns; track c.key) {
              <span role="columnheader" [class.r]="c.align === 'r'" [attr.aria-sort]="ariaSort(c.key)">
                <button type="button" (click)="store.sortBy(c.key)" [attr.aria-label]="'Sort by ' + c.label">{{ c.label }} @if (store.sort().key === c.key) {<dy-icon [name]="store.sort().dir === 'asc' ? 'arrow-up' : 'arrow-down'" />}</button>
              </span>
            }
            <span role="columnheader" class="act"></span>
          </div>
          @if (!store.connected()) {
            <div class="skeletons" aria-busy="true" aria-label="Loading processes">
              @for (i of placeholders; track i) {
                <div class="skeleton"></div>
              }
            </div>
          } @else if (store.rows().length === 0) {
            <div class="empty"><dy-icon name="search" /><strong>No process matches “{{ store.query() }}”.</strong><button type="button" class="btn sm" (click)="store.setQuery('')">Clear filter</button></div>
          } @else {
            <cdk-virtual-scroll-viewport itemSize="32" minBufferPx="320" maxBufferPx="640" class="vp" (keydown)="key($event)">
              <div
                *cdkVirtualFor="let r of store.rows(); trackBy: trackRow"
                class="row"
                role="row"
                tabindex="0"
                [attr.aria-level]="r.depth + 1"
                [attr.aria-expanded]="r.expandable ? r.expanded : null"
                [class.selected]="isSelected(r)"
                [class.suspended]="r.suspended"
                [class.group]="r.kind === 'group'"
                [attr.data-key]="r.key"
                [cdkContextMenuTriggerFor]="menu"
                [cdkContextMenuTriggerData]="{ row: r }"
                (click)="rowClick(r)"
                (keydown.space)="$event.preventDefault(); rowClick(r)"
                (dblclick)="store.toggle(r)"
              >
                <span role="gridcell" class="name" [style.padding-left.px]="8 + r.depth * 20">
                  @if (r.expandable) {
                    <button type="button" class="chev" [attr.aria-label]="(r.expanded ? 'Collapse ' : 'Expand ') + r.name" (click)="$event.stopPropagation(); store.toggle(r)">
                      <dy-icon [name]="r.expanded ? 'chevron-down' : 'chevron-right'" />
                    </button>
                  } @else {
                    <span class="chev-space"></span>
                  }
                  @if (r.protectedReason) {
                    <dy-icon name="lock" class="lock" [label]="'Protected: ' + r.protectedReason" />
                  }
                  <span class="nm truncate" [title]="r.name">{{ r.name }}</span>
                  @if (r.kind === 'group' && r.count > 1) {
                    <span class="badge">{{ num(r.count) }}</span>
                  }
                  @if (r.suspended) {
                    <span class="badge" data-tone="warn">Suspended</span>
                  }
                </span>
                <span role="gridcell" class="r num heat" [style.background]="heat(r.cpu, 40)">{{ pct(r.cpu) }}</span>
                <span role="gridcell" class="r num heat" [style.background]="heat(r.memory, 2147483648)">{{ bytes(r.memory) }}</span>
                <span role="gridcell" class="r num heat" [style.background]="heat(r.disk, 52428800)">{{ rate(r.disk) }}</span>
                <span role="gridcell" class="truncate faint">{{ r.user ?? '' }}</span>
                <span role="gridcell" class="r num faint">{{ pid(r) }}</span>
                <span role="gridcell" class="act">
                  <button type="button" class="btn ghost icon sm" [attr.aria-label]="'Actions for ' + r.name" [cdkMenuTriggerFor]="menu" [cdkMenuTriggerData]="{ row: r }" (click)="$event.stopPropagation()"><dy-icon name="more" /></button>
                </span>
              </div>
            </cdk-virtual-scroll-viewport>
          }
        </section>
        @if (store.selected()) {
          <dy-process-details />
        }
      </div>
    </div>

    <ng-template #menu let-row="row">
      <div class="menu wide" cdkMenu [attr.aria-label]="'Actions for ' + row.name">
        @if (row.protectedReason; as why) {
          <div class="menu-note"><dy-icon name="lock" /> <span>{{ why }}. Dyno never changes it.</span></div>
        }
        @if (row.ref) {
          <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="details(row)"><dy-icon name="info" /> Details</button>
        }
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!!row.protectedReason || store.busy()" (cdkMenuItemTriggered)="end(row)"><dy-icon name="stop" /> End…</button>
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!!row.protectedReason || store.busy()" (cdkMenuItemTriggered)="store.act(row, row.suspended ? 'resume' : 'suspend')">
          <dy-icon [name]="row.suspended ? 'play' : 'pause'" /> {{ row.suspended ? 'Resume' : 'Suspend' }}
        </button>
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!!row.protectedReason" [cdkMenuTriggerFor]="priority" [cdkMenuTriggerData]="{ row }"><dy-icon name="gauge" /> Priority <dy-icon name="chevron-right" class="trail" /></button>
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!!row.protectedReason" (cdkMenuItemTriggered)="affinity(row)"><dy-icon name="cpu" /> Choose cores…</button>
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!!row.protectedReason" (cdkMenuItemTriggered)="store.act(row, { setEfficiencyMode: true })"><dy-icon name="bolt" /> Efficiency mode on</button>
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!!row.protectedReason" (cdkMenuItemTriggered)="store.act(row, { setEfficiencyMode: false })"><dy-icon name="bolt" /> Efficiency mode off</button>
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!!row.protectedReason" (cdkMenuItemTriggered)="cap(row)"><dy-icon name="gauge" /> Cap CPU…</button>
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!!row.protectedReason" (cdkMenuItemTriggered)="store.act(row, { setCpuCap: null })"><dy-icon name="undo" /> Remove CPU cap</button>
        @if (row.ref) {
          <div class="menu-sep"></div>
          <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="store.openLocation(row.ref)"><dy-icon name="folder" /> Open file location</button>
        }
      </div>
    </ng-template>
    <ng-template #priority let-row="row">
      <div class="menu" cdkMenu aria-label="Priority">
        @for (p of priorities; track p) {
          <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="store.act(row, { setPriority: p })">{{ prioLabel(p) }}</button>
        }
      </div>
    </ng-template>
  `,
  styles: `
    :host { display: block; height: 100%; }
    .wrap { height: 100%; display: flex; flex-direction: column; padding: var(--sp-5) var(--sp-6) var(--sp-4); gap: var(--sp-3); min-height: 0; }
    .head { display: flex; align-items: center; gap: var(--sp-3); flex-wrap: wrap; }
    .titles { flex: 1; min-width: 200px; display: grid; gap: 2px; }
    .titles p { color: var(--text-3); }
    .search { display: flex; align-items: center; gap: var(--sp-2); height: var(--control-h); padding: 0 var(--sp-3); width: 260px; border: 1px solid var(--border-strong); border-radius: var(--r-md); background: var(--surface-2); color: var(--text-3); }
    .search:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft-2); }
    .search input { flex: 1; min-width: 0; border: 0; outline: none; background: none; color: var(--text-1); font: var(--fs-base) var(--font-sans); }
    .search dy-icon { --icon-size: 15px; }
    .banner { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) var(--sp-3); border: 1px solid color-mix(in srgb, var(--bad) 35%, transparent); border-radius: var(--r-md); background: var(--bad-soft); }
    .banner > dy-icon { color: var(--bad); }
    .banner span { flex: 1; } .banner small { display: block; color: var(--text-2); }
    .body { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); }
    .body.open { grid-template-columns: minmax(0, 1fr) 380px; }
    .table { display: flex; flex-direction: column; min-height: 0; min-width: 0; overflow: hidden; }
    .row { display: grid; grid-template-columns: minmax(200px, 1fr) 84px 96px 96px 130px 64px 36px; align-items: center; height: 32px; padding-right: var(--sp-2); border-bottom: 1px solid var(--border-subtle); font-size: var(--fs-base); outline: none; }
    .row > span { padding: 0 var(--sp-2); min-width: 0; }
    .row > .r { text-align: right; }
    .head-row { height: 32px; flex: none; border-bottom: 1px solid var(--border); background: var(--surface-2); }
    .head-row button { all: unset; display: inline-flex; align-items: center; gap: 4px; color: var(--text-3); font-size: var(--fs-xs); font-weight: var(--fw-semibold); letter-spacing: 0.03em; text-transform: uppercase; cursor: default; }
    .head-row button:hover { color: var(--text-1); } .head-row button:focus-visible { outline: 2px solid var(--focus); border-radius: 3px; }
    .head-row dy-icon { --icon-size: 12px; }
    .vp { flex: 1; min-height: 0; }
    .row:not(.head-row):hover { background: var(--surface-hover); }
    .row.selected { background: var(--accent-soft); box-shadow: inset 2px 0 0 var(--accent); }
    .row:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; border-radius: 0; }
    .row.group { font-weight: var(--fw-medium); }
    .row.suspended .nm { color: var(--text-3); }
    .name { display: flex; align-items: center; gap: 6px; }
    .nm { min-width: 0; }
    .chev { all: unset; display: grid; place-items: center; width: 18px; height: 18px; border-radius: 4px; color: var(--text-3); cursor: default; flex: none; }
    .chev:hover { background: var(--surface-active); } .chev:focus-visible { outline: 2px solid var(--focus); }
    .chev dy-icon { --icon-size: 14px; } .chev-space { width: 18px; flex: none; }
    .lock { --icon-size: 13px; color: var(--text-3); flex: none; }
    .heat { border-radius: 0; }
    .faint { color: var(--text-3); }
    .act { padding: 0 !important; display: grid; place-items: center; }
    .skeletons { display: grid; gap: var(--sp-2); padding: var(--sp-4); } .skeleton { height: 20px; }
    .empty { display: grid; justify-items: center; gap: var(--sp-2); padding: var(--sp-10); color: var(--text-2); }
    .menu-note { display: flex; gap: var(--sp-2); padding: var(--sp-2); color: var(--text-2); font-size: var(--fs-sm); white-space: normal; border-bottom: 1px solid var(--border); margin-bottom: var(--sp-1); }
  `,
})
export class ProcessesPage implements OnInit, OnDestroy {
  protected readonly store = inject(ProcessesStore);
  private readonly dialogs = inject(Dialogs);
  private readonly live = inject(LiveStore);
  /** `?q=` filters on open (from the dashboard's top apps and the command palette). */
  readonly q = input<string>();

  protected readonly columns = COLUMNS;
  protected readonly priorities = PRIORITIES;
  protected readonly placeholders = Array.from({ length: 12 }, (_, i) => i);
  protected readonly modes: { id: ViewMode; label: string; icon: 'layers' | 'rules' | 'processes' }[] = [
    { id: 'groups', label: 'Apps', icon: 'layers' },
    { id: 'tree', label: 'Tree', icon: 'rules' },
    { id: 'flat', label: 'All', icon: 'processes' },
  ];
  protected readonly pct = formatPercent;
  protected readonly bytes = formatBytes;
  protected readonly rate = formatRate;
  protected readonly heat = heat;
  protected readonly prioLabel = (p: Priority) => PRIORITY_LABEL[p];
  protected readonly hint = computed(() => {
    const e = this.store.actionError();
    return e ? errorHint(e.code) : null;
  });
  protected readonly summary = computed(() => {
    const s = this.live.system();
    const n = this.store.total();
    return n === 0 ? 'Reading processes…' : `${plural(n, 'process', 'processes')}${s ? ` · ${formatPercent(s.cpu.total)} CPU · ${formatBytes(s.memory.used)} memory in use` : ''}`;
  });
  protected readonly trackRow = (_: number, r: Row) => r.key;
  protected readonly num = formatCount;
  protected readonly pid = (r: Row) => (r.pid === null ? '' : String(r.pid));
  private readonly cores = computed(() => this.live.system()?.cpu.perCore.length ?? 1);

  constructor() {
    effect(() => {
      const q = this.q();
      if (q !== undefined) this.store.setQuery(q);
    });
  }

  ngOnInit(): void {
    this.store.start();
  }
  ngOnDestroy(): void {
    this.store.stop();
  }

  protected ariaSort(key: SortKey): string | null {
    const s = this.store.sort();
    return s.key === key ? (s.dir === 'asc' ? 'ascending' : 'descending') : null;
  }
  protected isSelected(r: Row): boolean {
    return r.ref !== null && this.store.selected()?.pid === r.ref.pid;
  }
  protected rowClick(r: Row): void {
    if (r.ref) void this.store.select(r.ref);
    else this.store.toggle(r);
  }
  protected details(r: Row): void {
    if (r.ref) void this.store.select(r.ref);
  }

  protected async end(r: Row): Promise<void> {
    const group = r.kind === 'group';
    const ok = await this.dialogs.confirm({
      title: group ? `End all of ${r.name}?` : `End ${r.name}?`,
      message: group ? `This ends ${plural(r.count, 'process', 'processes')}. Unsaved work in the app is lost.` : 'Unsaved work in it is lost.',
      confirmLabel: group ? 'End all' : 'End',
      danger: true,
      details: ['Ending cannot be undone.', 'Protected system processes are never touched.'],
    });
    if (ok) await this.store.act(r, 'end');
  }

  protected async affinity(r: Row): Promise<void> {
    const mask = this.store.details()?.process.pid === r.pid ? this.store.details()!.affinity : null;
    const ref = this.dialogs.open<number>(AffinityDialog, { name: r.name, cores: this.cores(), mask });
    const result = await firstValueFrom(ref.closed);
    if (typeof result === 'number') await this.store.act(r, { setAffinity: result });
  }

  protected async cap(r: Row): Promise<void> {
    const ref = this.dialogs.open<number>(CapDialog, r.name);
    const result = await firstValueFrom(ref.closed);
    if (typeof result === 'number') await this.store.act(r, { setCpuCap: result });
  }

  protected lockers(): void {
    this.dialogs.open(LockersDialog);
  }

  /** Arrow keys move between rows, Right/Left open and close, Enter shows details, Delete ends. */
  protected key(e: KeyboardEvent): void {
    const el = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>('[role=row]') : null;
    if (!el) return;
    const row = this.store.rows().find((r) => r.key === el.dataset['key']);
    if (!row) return;
    const move = (to: Element | null) => {
      if (to instanceof HTMLElement) {
        e.preventDefault();
        to.focus();
      }
    };
    switch (e.key) {
      case 'ArrowDown':
        return move(el.nextElementSibling);
      case 'ArrowUp':
        return move(el.previousElementSibling);
      case 'ArrowRight':
        if (row.expandable && !row.expanded) this.store.toggle(row);
        return;
      case 'ArrowLeft':
        if (row.expandable && row.expanded) this.store.toggle(row);
        return;
      case 'Enter':
        e.preventDefault();
        return this.rowClick(row);
      case 'Delete':
        if (!row.protectedReason) void this.end(row);
        return;
    }
  }
}
