import { PdCell, PdTable, PdTableEmpty, type PdColumn, type PdRowAction, type PdRowActionEvent, type PdRowTree, type PdSort } from '@decryp7/paddock-ui/table';
import { FullTextTip } from '../../shared/full-text-tip';
import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, OnDestroy, OnInit, afterNextRender, computed, effect, inject, input, linkedSignal, signal, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';
import type { CommandError } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { formatBytes, formatCount, formatPercent, formatRate, plural } from '../../shared/format';
import { bar } from '../../shared/heat';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { ReadingSource } from '../../core/reading-source';
import { LiveStore } from '../../state/live.store';
import { Lookups } from '../lookup/lookups';
import { ProcessesStore } from '../../state/processes.store';
import { ProcessDetailsPanel } from './process-details';
import { type Row, type SortKey, type ViewMode } from './process-rows';
import { MatButton, MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';
import { SearchField } from '../../shared/search-field';
import { TreeActions } from '../../shared/tree-actions';
import { MatMenuModule } from '@angular/material/menu';

/** What the table spends around its rows: its header and its frame. */
const TABLE_CHROME = 28 + 2 + 16 + 8;

/** The task manager: every app and process, grouped by app, as a tree or flat; sortable, searchable, with a details panel and lookups. Read-only: nothing here changes a process. */
@Component({
  selector: 'dy-processes-page',
  imports: [ReadingSource, FullTextTip, AsyncState, Icon, FormsModule, PdTable, PdCell, PdTableEmpty, ProcessDetailsPanel, MatMenuModule, SearchField, MatTooltip, SuSegmentedControl, MatButton, MatIconButton, TreeActions],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="wrap">
      <header class="head">
        <div class="titles">
          <h1>Processes</h1>
          <p class="num">{{ summary() }}</p>
        </div>
        <pd-segmented-control label="View" [options]="modeSegs()" [value]="store.mode()" (valueChange)="pickMode($event)" />
        <dy-search-field label="Filter processes" placeholder="Filter by name, account or PID" [value]="store.query()" (valueChange)="store.setQuery($event)" />
        <dy-tree-actions what="processes" [disabled]="store.mode() === 'flat'" (expandAll)="store.expandAll()" (collapseAll)="store.collapseAll()" />
        <button type="button" matButton="outlined" (click)="lockers()"><dy-icon name="lock" /> What is locking this file?</button>
      </header>

      @if (store.actionError(); as e) {
        <div class="banner" role="alert">
          <dy-icon name="alert" />
          <span><strong>{{ e.message }}</strong> @if (hint(); as h) {<small>{{ h }}</small>}</span>
          <button type="button" matIconButton matTooltip="Dismiss" aria-label="Dismiss" (click)="store.dismissError()"><dy-icon name="close" /></button>
        </div>
      }

      @if (store.otherUsers(); as o) {
        <p class="note" data-testid="other-users-note">
          <dy-icon name="info" /> “Other users” is one row for the {{ num(o.count) }} processes of other accounts on this computer.
          @if (store.via()?.others === 'helper') { It is counted by the administrator helper. } @else { It adds up only the values Dyno can read. }
        </p>
      }
      <div class="source" data-testid="processes-source"><dy-reading-source [via]="store.via()?.processes" [canRetry]="true" (retry)="retry()" /></div>

      <div class="body" [class.open]="!!store.selected()">
        <section class="table card" #frame>
          @if (agentError(); as e) {
            <dy-async [loaded]="false" [error]="e" what="processes" (retry)="retry()" />
          } @else {
          <pd-table
            label="Processes"
            order="source"
            [toolbar]="false"
            [tree]="tree"
            [rows]="store.rows()"
            [columns]="columns"
            [rowKey]="rowKey"
            [filterable]="false"
            [state]="store.connected() ? 'ready' : 'loading'"
            [maxHeight]="bodyMax()"
            [rowActions]="actions"
            [sortClearable]="false"
            [sort]="pdSort()"
            (sortChange)="onSort($event)"
            [(selectedKey)]="selectedKey"
            (rowActivate)="rowClick($event)"
            (rowToggle)="store.toggle($event)"
            (action)="run($event)"
            (keydown.escape)="closeDetails($event)"
          >
            <ng-template pdCell="name" let-r>
              <span class="nm-cell" [class.suspended]="r.suspended">
                @if (r.expandable) {
                  <button type="button" class="chev" tabindex="-1" [attr.aria-label]="(r.expanded ? 'Collapse ' : 'Expand ') + r.name" (click)="$event.stopPropagation(); store.toggle(r)">
                    <dy-icon [name]="r.expanded ? 'chevron-down' : 'chevron-right'" />
                  </button>
                } @else {
                  <span class="chev-space"></span>
                }
                <span class="nm truncate" [dyFullText]="r.name">{{ r.name }}</span>
                @if (r.kind !== 'process' && r.count > 1) {
                  <span class="badge">{{ num(r.count) }}</span>
                }
                @if (r.suspended) {
                  <span class="badge" data-tone="warn">Suspended</span>
                }
              </span>
            </ng-template>
            <ng-template pdCell="cpu" let-r>
              <span class="heat" [style.background]="bar(r.cpu ?? 0, 40, sortKey() === 'cpu')">
                @if (r.cpu === null) {
                  <span class="na" [title]="r.unavailable.cpu" [attr.aria-label]="r.unavailable.cpu">Unavailable</span>
                } @else {
                  <span [title]="r.partial ? 'Partial: some values could not be read' : ''">{{ pct(r.cpu) }}{{ r.partial ? ' (partial)' : '' }}</span>
                }
              </span>
            </ng-template>
            <ng-template pdCell="memory" let-r>
              <span class="heat" [style.background]="bar(r.memory ?? 0, 2147483648, sortKey() === 'memory')">
                @if (r.memory === null) {
                  <span class="na" [title]="r.unavailable.memory" [attr.aria-label]="r.unavailable.memory">Unavailable</span>
                } @else {
                  <span [title]="r.partial ? 'Partial: some values could not be read' : ''">{{ bytes(r.memory) }}{{ r.partial ? ' (partial)' : '' }}</span>
                }
              </span>
            </ng-template>
            <ng-template pdCell="disk" let-r>
              <span class="heat" [style.background]="bar(r.disk ?? 0, 52428800, sortKey() === 'disk')">
                @if (r.disk === null) {
                  <span class="na" [title]="r.unavailable.disk" [attr.aria-label]="r.unavailable.disk">Unavailable</span>
                } @else {
                  <span [title]="r.partial ? 'Partial: some values could not be read' : ''">{{ rate(r.disk) }}{{ r.partial ? ' (partial)' : '' }}</span>
                }
              </span>
            </ng-template>
            <div pdTableEmpty class="empty"><dy-icon name="search" /><strong>No process matches “{{ store.query() }}”.</strong><button type="button" matButton="outlined" (click)="store.setQuery('')">Clear filter</button></div>
          </pd-table>
          }
        </section>
        @if (store.selected()) {
          <dy-process-details />
        }
      </div>
    </div>
  `,
  styles: `
    :host { display: block; height: 100%; }
    .chev { width: 22px; height: 22px; padding: 0; flex: none; --icon-size: 14px; }
    .wrap { height: 100%; display: flex; flex-direction: column; padding: var(--sp-5) var(--sp-6) var(--sp-4); gap: var(--sp-3); min-height: 0; }
    .source { display: contents; }
    .head { display: flex; align-items: center; gap: var(--sp-3); flex-wrap: wrap; }
    .titles { flex: 1; min-width: 200px; display: grid; gap: 2px; }
    .titles p { color: var(--text-3); }
    .banner { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) var(--sp-3); border: 1px solid color-mix(in srgb, var(--bad) 35%, transparent); border-radius: var(--r-md); background: var(--bad-soft); }
    .banner > dy-icon { color: var(--bad); }
    .banner span { flex: 1; } .banner small { display: block; color: var(--text-2); }
    .body { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); }
    .body.open { grid-template-columns: minmax(0, 1fr) 380px; }
    /* Under 1100 px the panel goes below the table (the table needs about 610 px), as on the alarms page. */
    @media (max-width: 1100px) { .body.open { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr) minmax(240px, 45%); } }
    .table { min-height: 0; min-width: 0; overflow: hidden; padding: var(--sp-2); }
    .nm-cell { display: flex; align-items: center; gap: 6px; min-width: 0; flex: 1; }
    .nm { min-width: 0; }
    .truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .suspended .nm { color: var(--text-3); }
    .chev { all: unset; display: grid; place-items: center; width: 18px; height: 18px; border-radius: 4px; color: var(--text-3); cursor: default; flex: none; }
    .chev:hover { background: var(--surface-active); }
    .chev dy-icon { --icon-size: 14px; } .chev-space { width: 18px; flex: none; }
    .heat { display: flex; align-items: center; justify-content: flex-end; flex: 1; align-self: stretch; min-width: 0; padding-inline: var(--sp-2); font-variant-numeric: tabular-nums; }
    .faint { color: var(--text-3); }
    .note { display: flex; align-items: center; gap: var(--sp-2); margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .na { color: var(--text-3); font-style: italic; }
    .empty { display: grid; justify-items: center; gap: var(--sp-2); padding: var(--sp-10); color: var(--text-2); }
  `,
})
export class ProcessesPage implements OnInit, OnDestroy {
  protected readonly store = inject(ProcessesStore);
  private readonly dialogs = inject(DialogService);
  private readonly live = inject(LiveStore);
  private readonly lookups = inject(Lookups);
  private readonly grid = viewChild(PdTable);
  /** `?q=` filters on open (from the dashboard's top apps and the command palette). */
  readonly q = input<string>();

  protected pickMode(value: string): void {
    const m = this.modes.find((x) => x.id === value);
    if (m) this.store.setMode(m.id);
  }
  protected readonly modeSegs = computed<SuSegment[]>(() => this.modes.map((m) => ({ value: m.id, label: m.label })));
  protected readonly modes: { id: ViewMode; label: string; icon: 'layers' | 'rules' | 'processes' }[] = [
    { id: 'groups', label: 'Apps', icon: 'layers' },
    { id: 'tree', label: 'Tree', icon: 'rules' },
    { id: 'flat', label: 'All', icon: 'processes' },
  ];
  protected readonly pct = formatPercent;
  protected readonly bytes = formatBytes;
  protected readonly rate = formatRate;
  protected readonly bar = bar;
  protected readonly sortKey = computed(() => this.store.sort().key);
  protected readonly hint = computed(() => {
    const e = this.store.actionError();
    return e ? errorHint(e.code) : null;
  });
  protected readonly summary = computed(() => {
    const s = this.live.system();
    const n = this.store.total();
    return n === 0 ? 'Reading processes…' : `${plural(n, 'process', 'processes')}${s ? ` · ${formatPercent(s.cpu.total)} CPU · ${formatBytes(s.memory.used)} memory in use` : ''}`;
  });
  protected readonly num = formatCount;
  protected readonly rowKey = (r: Row): string => r.key;
  /** The columns are the store's sort keys: the table shows the sort and reports a click, the store orders the tree. */
  protected readonly columns: PdColumn<Row>[] = [
    { key: 'name', header: 'Name', value: (r) => r.name, required: true, width: 'minmax(200px, 1fr)' },
    { key: 'cpu', sortStart: 'desc', header: 'CPU', value: (r) => r.cpu, align: 'end', width: '84px' },
    { key: 'memory', sortStart: 'desc', header: 'Memory', value: (r) => r.memory, align: 'end', width: '96px' },
    { key: 'disk', sortStart: 'desc', header: 'Disk', value: (r) => r.disk, align: 'end', width: '96px' },
    { key: 'user', header: 'Account', value: (r) => r.user, width: '130px', hideBelow: 760 },
    { key: 'pid', header: 'PID', value: (r) => r.pid, text: (r) => (r.pid === null ? '' : String(r.pid)), align: 'end', width: '64px', hideBelow: 680 },
  ];
  /** Apps, processes and their children: the table indents, folds and says the levels. */
  protected readonly tree: PdRowTree<Row> = { level: (r) => r.depth + 1, expanded: (r) => (r.expandable ? r.expanded : null) };
  protected readonly pdSort = computed<PdSort>(() => ({ key: this.store.sort().key, direction: this.store.sort().dir }));
  /** The highlighted row follows the open details panel (a click on a group folds it and leaves the highlight where it was). */
  protected readonly selectedKey = linkedSignal<string | null>(() => this.rows().find((r) => this.isSelected(r))?.key ?? null);
  private readonly rows = this.store.rows;
  private readonly frame = viewChild<ElementRef<HTMLElement>>('frame');
  private readonly frameHeight = signal(0);
  /** The rows scroll inside the table, which fills the pane: what is left under the table's own toolbar and header. */
  protected readonly bodyMax = computed(() => Math.max(160, this.frameHeight() - TABLE_CHROME));
  /** The row actions; "Other users" is one row for many accounts and has none. */
  protected readonly actions = (r: Row): PdRowAction[] => {
    if (r.kind === 'other') return [];
    return [
      ...(r.ref ? [{ id: 'details', label: 'Details', icon: 'info' }, { id: 'location', label: 'Open file location', icon: 'folder' }] : []),
      { id: 'web', label: 'Search the web…', icon: 'search' },
      ...(r.ref ? [{ id: 'hash', label: 'Search file hash on VirusTotal…', icon: 'shield' }] : []),
    ];
  };

  constructor() {
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver((entries) => this.frameHeight.set(Math.round(entries[0]?.contentRect.height ?? 0)));
    afterNextRender(() => {
      const el = this.frame()?.nativeElement;
      if (el) observer?.observe(el);
    });
    inject(DestroyRef).onDestroy(() => observer?.disconnect());
    effect(() => {
      const q = this.q();
      if (q !== undefined) this.store.setQuery(q);
    });
  }

  /** The agent stopped before any process list came: the table says so, with Retry (no list will come by itself). */
  protected readonly agentError = computed<CommandError | null>(() => {
    const a = this.live.agent();
    return a.state === 'failed' && !this.store.connected() ? { code: 'agentUnavailable', message: a.message ?? 'Dyno’s background agent stopped, so there is no process list.' } : null;
  });
  protected retry(): void {
    this.store.stop();
    this.store.start();
  }

  ngOnInit(): void {
    this.store.start();
  }
  ngOnDestroy(): void {
    this.store.stop();
  }

  /** A header click: the store sorts the tree (always ascending or descending: the list is never unsorted). */
  protected onSort(sort: PdSort | null): void {
    if (sort) this.store.setSort({ key: sort.key as SortKey, dir: sort.direction });
  }
  protected isSelected(r: Row): boolean {
    return r.ref !== null && this.store.selected()?.pid === r.ref.pid;
  }
  protected rowClick(r: Row): void {
    if (r.ref) void this.store.select(r.ref);
    else {
      this.store.toggle(r);
      this.selectedKey.set(this.rows().find((x) => this.isSelected(x))?.key ?? null);
    }
  }

  /** Escape closes the details panel and keeps the focus on the row that was open. */
  protected closeDetails(e: Event): void {
    const key = this.selectedKey();
    if (!this.store.selected() || key === null) return;
    e.preventDefault();
    void this.store.select(null);
    this.grid()?.focusRow(key);
  }

  protected run({ id, row }: PdRowActionEvent<Row>): void {
    if (id === 'details') this.details(row);
    else if (id === 'location' && row.ref) void this.store.openLocation(row.ref);
    else if (id === 'web') this.searchWeb(row);
    else this.searchHash(row);
  }
  protected details(r: Row): void {
    if (r.ref) void this.store.select(r.ref);
  }

  protected searchWeb(r: Row): void {
    void this.lookups.searchWeb(r.name, r.name);
  }

  protected searchHash(r: Row): void {
    if (r.ref) void this.lookups.searchHash({ process: r.ref }, r.name);
  }

  protected lockers(): void {
    void this.dialogs.open('dy.lockers').closed;
  }
}
