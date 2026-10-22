import { CdkFixedSizeVirtualScroll, CdkVirtualForOf, CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, DestroyRef, OnInit, computed, inject, signal } from '@angular/core';
import type { CategoryResult, CleanupCategory, CleanupItem } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Dialogs } from '../../shared/dialog';
import { formatBytes, formatRelative, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { GROUP_ICON, GROUP_LABEL, RISK_LABEL } from '../../shared/labels';
import { Notifier } from '../../shared/notifier';
import { CleanupStore } from '../../state/cleanup.store';
import { CleanupConfirmDialog, type CleanupConfirmData } from './cleanup-confirm-dialog';
import { baseName, groupCategories, isStale, outcomeSentence, selectionState, summarise } from './cleanup-model';
import type { DeleteMode } from '../../core/api.types';

/** Lists longer than this scroll in a virtualised viewport. */
export const VIRTUAL_FROM = 60;
const ROW_PX = 44;
const TICK_MS = 30_000;

/**
 * Cleanup: scan (read-only) → review every item → confirm in a dialog → result with undo. Nothing is deleted by
 * scanning, and nothing runs without the dialog. The engine re-checks each item and refuses an old scan.
 */
@Component({
  selector: 'dy-cleanup-page',
  imports: [AsyncState, Icon, NgTemplateOutlet, CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Cleanup</h1>
          <p>Scan, review every item, then confirm. Scanning deletes nothing; cleaned items wait 7 days in quarantine so you can put them back.</p>
        </div>
        <button type="button" class="btn primary" [disabled]="store.scanning() || store.picked().length === 0" (click)="store.runScan()">
          <dy-icon name="search" />{{ store.scanning() ? 'Scanning…' : 'Scan ' + plural(store.picked().length, 'category', 'categories') }}
        </button>
      </header>

      <dy-async [loaded]="!!store.categories()" [error]="store.categoriesError()" what="cleanup categories" [rows]="5" (retry)="store.loadCategories()">
        <section class="card" aria-label="What to scan">
          <div class="card-head"><h2>What to scan</h2></div>
          @for (g of groups(); track g.group) {
            <div class="group" role="group" [attr.aria-label]="groupLabel(g.group)">
              <h3><dy-icon [name]="groupIcon(g.group)" /> {{ groupLabel(g.group) }}</h3>
              @for (c of g.categories; track c.id) {
                <label class="cat">
                  <input type="checkbox" [checked]="store.picked().includes(c.id)" (change)="store.togglePick(c.id)" />
                  <span class="txt"><strong>{{ c.name }}</strong><span class="sub">{{ c.description }}</span></span>
                  <span class="badge outline">{{ risk(c) }}</span>
                </label>
              }
            </div>
          }
        </section>
      </dy-async>

      @if (store.scanError(); as e) {
        <div class="note bad" role="alert">
          <dy-icon name="alert" /><span class="msg">{{ e.message }}</span>
          <button type="button" class="btn sm" (click)="store.runScan()"><dy-icon name="refresh" /> Retry</button>
        </div>
      }

      @if (store.result(); as r) {
        <section class="card result" aria-label="Result" role="status">
          <div class="card-head">
            <h2><dy-icon name="check" /> {{ sentence() }}</h2>
            <span class="spacer"></span>
            @if (r.outcome.quarantineId; as q) {
              <button type="button" class="btn" [disabled]="store.restoring() !== null" (click)="store.restore(q)"><dy-icon name="undo" /> Undo</button>
            }
            <button type="button" class="btn sm ghost icon" aria-label="Dismiss the result" (click)="store.dismissResult()"><dy-icon name="close" /></button>
          </div>
          <p class="plain">{{ r.mode === 'quarantine' ? 'Disk space is freed when the quarantine ends after 7 days.' : 'Disk space is freed when you empty the Recycle Bin / Trash.' }}</p>
          @if (r.outcome.failed.length) {
            <div class="failed">
              <h3>{{ plural(r.outcome.failed.length, 'item') }} skipped</h3>
              <ul>
                @for (f of failedShown(); track f.path) {
                  <li><span class="mono ell" [title]="f.path">{{ f.path }}</span><span class="why">{{ f.reason }}</span></li>
                }
              </ul>
              @if (r.outcome.failed.length > failedShown().length) {
                <p class="plain">and {{ r.outcome.failed.length - failedShown().length }} more.</p>
              }
            </div>
          }
        </section>
      }

      @if (store.scan(); as scan) {
        <section class="card" aria-label="Preview">
          <div class="card-head">
            <h2>Review</h2>
            <span class="sub">{{ plural(totalItems(), 'item') }} found · {{ bytes(scan.totalBytes) }}</span>
            <span class="spacer"></span>
            <button type="button" class="btn sm ghost" (click)="store.clearSelection()" [disabled]="store.selection().length === 0">Clear selection</button>
          </div>
          @if (stale()) {
            <p class="note warn" role="alert"><dy-icon name="alert" /> This scan is more than an hour old. Scan again before cleaning.</p>
          }
          @for (r of scan.results; track r.category) {
            <div class="result-cat" role="group" [attr.aria-label]="name(r.category)">
              <div class="cat-head">
                <input type="checkbox" [attr.aria-label]="'Select all in ' + name(r.category)" [checked]="state(r) === 'all'" [indeterminate]="state(r) === 'some'" [disabled]="r.items.length === 0" (change)="store.setCategory(r.category, state(r) !== 'all')" />
                <strong>{{ name(r.category) }}</strong>
                <span class="sub">{{ plural(r.itemCount, 'item') }} · {{ bytes(r.bytes) }}</span>
              </div>
              @if (r.note) {
                <p class="plain"><dy-icon name="info" /> {{ r.note }}</p>
              }
              @if (r.truncated) {
                <p class="plain"><dy-icon name="info" /> Showing the {{ r.items.length }} biggest of {{ r.itemCount }}. Only listed items can be cleaned.</p>
              }
              @if (r.items.length === 0 && !r.note) {
                <p class="plain">Nothing to clean here.</p>
              }
              @if (r.items.length > virtualFrom) {
                <cdk-virtual-scroll-viewport class="viewport" [itemSize]="rowPx" [style.height.px]="viewportHeight" [attr.aria-label]="name(r.category) + ' items'">
                  <div *cdkVirtualFor="let i of r.items; trackBy: trackItem" class="item">
                    <ng-container *ngTemplateOutlet="row; context: { $implicit: i }" />
                  </div>
                </cdk-virtual-scroll-viewport>
              } @else {
                @for (i of r.items; track i.id) {
                  <div class="item">
                    <ng-container *ngTemplateOutlet="row; context: { $implicit: i }" />
                  </div>
                }
              }
            </div>
          }
          <div class="bar">
            <span class="summary" aria-live="polite">{{ plural(summary().count, 'item') }} selected · {{ bytes(summary().bytes) }}</span>
            <span class="spacer"></span>
            @if (stale()) {
              <button type="button" class="btn primary" (click)="store.runScan()"><dy-icon name="refresh" /> Scan again</button>
            } @else {
              <button type="button" class="btn primary" [disabled]="summary().count === 0 || store.running()" (click)="confirm()"><dy-icon name="cleanup" />{{ store.running() ? 'Moving…' : 'Clean selected…' }}</button>
            }
          </div>
        </section>
      }

      <section class="card" aria-label="Quarantine">
        <div class="card-head"><h2>Quarantine</h2><span class="sub">Cleaned items wait here for 7 days</span></div>
        <dy-async [loaded]="!!store.quarantines()" [error]="store.quarantineError()" [empty]="store.quarantines()?.length === 0" emptyIcon="check" emptyTitle="Nothing is waiting" emptyText="Items you clean to the quarantine appear here until the week is over." what="the quarantine" [rows]="2" (retry)="store.loadQuarantines()">
          <table class="table">
            <caption class="visually-hidden">Quarantines</caption>
            <thead><tr><th>Cleaned</th><th class="r">Items</th><th class="r">Size</th><th>Deleted for good</th><th><span class="visually-hidden">Actions</span></th></tr></thead>
            <tbody>
              @for (q of store.quarantines(); track q.id) {
                <tr>
                  <td>{{ ago(q.at) }}</td>
                  <td class="r num">{{ q.itemCount }}</td>
                  <td class="r num">{{ bytes(q.bytes) }}</td>
                  <td>{{ ago(q.expires) }}</td>
                  <td class="r"><button type="button" class="btn sm" [disabled]="store.restoring() !== null" (click)="store.restore(q.id)"><dy-icon name="restore" /> Put back</button></td>
                </tr>
              }
            </tbody>
          </table>
        </dy-async>
      </section>
    </div>

    <ng-template #row let-i>
      <label class="line">
        <input type="checkbox" [checked]="selected().has(i.id)" [attr.aria-label]="'Clean ' + base(i.path)" (change)="store.toggleItem(i.id)" />
        <dy-icon [name]="i.folder ? 'folder' : 'file'" />
        <span class="path mono ell" [title]="i.path">{{ i.path }}</span>
        <span class="why ell" [title]="i.reason">{{ i.reason }}</span>
        <span class="num size">{{ bytes(i.bytes) }}</span>
      </label>
    </ng-template>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .page-head .btn { align-self: start; }
    .group { padding: var(--sp-3) var(--sp-4) var(--sp-2); }
    .group + .group { border-top: 1px solid var(--border-subtle); }
    h3 { display: flex; align-items: center; gap: var(--sp-2); margin: 0 0 var(--sp-2); font: var(--fw-semibold) var(--fs-base) / 1.3 var(--font-sans); color: var(--text-2); }
    .cat { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: var(--sp-3); align-items: center; padding: 6px 0; cursor: pointer; }
    .txt { display: grid; min-width: 0; }
    .sub { color: var(--text-3); font-size: var(--fs-sm); }
    .note { display: flex; align-items: center; gap: var(--sp-2); margin: 0; padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); border-left: 3px solid var(--border-strong); background: var(--surface-2); font-size: var(--fs-sm); }
    .note.bad { border-left-color: var(--bad); background: var(--bad-soft); }
    .note.warn { margin: var(--sp-3) var(--sp-4) 0; border-left-color: var(--warn); background: var(--warn-soft); }
    .note .msg { flex: 1; min-width: 0; }
    .plain { display: flex; gap: var(--sp-2); align-items: center; margin: 0; padding: var(--sp-1) var(--sp-4); color: var(--text-2); font-size: var(--fs-sm); }
    .result .plain { padding-bottom: var(--sp-3); }
    .failed { padding: 0 var(--sp-4) var(--sp-3); }
    .failed ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); font-size: var(--fs-sm); }
    .failed li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--sp-4); }
    .why { color: var(--text-2); font-size: var(--fs-sm); }
    .result-cat { padding: var(--sp-3) 0 0; border-top: 1px solid var(--border-subtle); margin-top: var(--sp-3); }
    .cat-head { display: flex; align-items: center; gap: var(--sp-3); padding: 0 var(--sp-4) var(--sp-2); }
    .item { height: 44px; }
    .line { display: grid; grid-template-columns: auto auto minmax(0, 1fr) minmax(120px, 28%) 84px; gap: var(--sp-3); align-items: center; height: 44px; padding: 0 var(--sp-4); cursor: pointer; }
    .line:hover { background: var(--surface-hover); }
    .line dy-icon { --icon-size: 16px; color: var(--text-3); }
    .path { font-size: 12px; }
    .size { text-align: right; font-variant-numeric: tabular-nums; }
    .viewport { display: block; }
    .bar { position: sticky; bottom: 0; display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-3) var(--sp-4); border-top: 1px solid var(--border); background: var(--surface-2); border-radius: 0 0 var(--r-lg) var(--r-lg); }
    .summary { font-variant-numeric: tabular-nums; }
    .spacer { flex: 1; }
    .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .num { font-variant-numeric: tabular-nums; white-space: nowrap; }
    .table { margin: var(--sp-2) var(--sp-4) var(--sp-3); width: calc(100% - 2 * var(--sp-4)); }
    @media (max-width: 760px) { .line { grid-template-columns: auto auto minmax(0, 1fr) 72px; } .line .why { display: none; } }
  `,
})
export class CleanupPage implements OnInit {
  protected readonly store = inject(CleanupStore);
  private readonly dialogs = inject(Dialogs);
  private readonly notifier = inject(Notifier);
  private readonly destroyRef = inject(DestroyRef);
  protected readonly plural = plural;
  protected readonly bytes = formatBytes;
  protected readonly base = baseName;
  protected readonly virtualFrom = VIRTUAL_FROM;
  protected readonly rowPx = ROW_PX;
  protected readonly viewportHeight = ROW_PX * 8;
  protected readonly groups = computed(() => groupCategories(this.store.categories() ?? []));
  protected readonly selected = computed(() => new Set(this.store.selection()));
  protected readonly summary = computed(() => summarise(this.store.scan(), this.selected()));
  protected readonly totalItems = computed(() => (this.store.scan()?.results ?? []).reduce((a, r) => a + r.itemCount, 0));
  private readonly now = signal(Date.now());
  protected readonly stale = computed(() => isStale(this.store.scan(), this.now()));
  protected readonly sentence = computed(() => {
    const r = this.store.result();
    return r ? outcomeSentence(r.outcome, r.mode) : '';
  });
  protected readonly failedShown = computed(() => this.store.result()?.outcome.failed.slice(0, 10) ?? []);

  ngOnInit(): void {
    // So the "scan is old" note appears while the page sits open.
    const tick = setInterval(() => this.now.set(Date.now()), TICK_MS);
    this.destroyRef.onDestroy(() => clearInterval(tick));
    void this.store.loadCategories();
    void this.store.loadQuarantines();
  }

  protected groupLabel = (g: CleanupCategory['group']) => GROUP_LABEL[g];
  protected groupIcon = (g: CleanupCategory['group']) => GROUP_ICON[g];
  protected risk = (c: CleanupCategory) => RISK_LABEL[c.risk];
  protected ago = (at: number) => formatRelative(at, Date.now());
  protected state = (r: CategoryResult) => selectionState(r, this.selected());
  protected trackItem = (_: number, i: CleanupItem) => i.id;

  protected name(id: string): string {
    return this.store.categories()?.find((c) => c.id === id)?.name ?? id;
  }

  /** Asks the last question; only a confirmed answer reaches the engine. */
  protected async confirm(): Promise<void> {
    if (isStale(this.store.scan(), Date.now())) {
      this.notifier.show('This scan is more than an hour old. Scan again before cleaning.', 'warn');
      return;
    }
    const s = this.summary();
    const data: CleanupConfirmData = {
      count: s.count,
      bytes: s.bytes,
      rows: s.byCategory.map((c) => ({ name: this.name(c.category), count: c.count, bytes: c.bytes })),
      mode: 'quarantine',
    };
    const ref = this.dialogs.open<DeleteMode | null, CleanupConfirmData>(CleanupConfirmDialog, data);
    const mode = await new Promise<DeleteMode | null>((resolve) => ref.closed.subscribe((m) => resolve(m ?? null)));
    if (mode) await this.store.run(mode);
  }
}
