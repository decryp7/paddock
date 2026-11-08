import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatProgressBar } from '@angular/material/progress-bar';
import { MatTooltip } from '@angular/material/tooltip';
import { PdPathText } from '@decryp7/paddock-ui';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { formatStorage } from '@decryp7/paddock-ui/format';
import { PdCell, PdMiddleText, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent } from '@decryp7/paddock-ui/table';
import { answerOf } from '@decryp7/paddock-ui/shell';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { fileIcon } from '@decryp7/paddock-ui/shell/files';
import { STORAGE_CONFIRM_DIALOG_ID, StorageStore, floorBytes, inSentence, treeTable, type StorageCategory, type TreeRow } from '@decryp7/paddock-ui/shell/storage';
import { STORAGE_STYLES } from './styles';

/**
 * Settings > Storage (plan X32): what the app keeps on disk, one row per category with its folder, its size ("at least" when
 * the walk was cut short) and what clearing would remove. The cache is cleared together ("Clear cache"); the user's own data
 * (logs, models, stores) one category at a time. Every clear asks first in a modal window of its own and shows what it did
 * afterwards, including what it left in place and why. A scan and a clear run as jobs: the page shows the phase, "Still
 * working" and Cancel for as long as they run (rules 11 and 12), and a failed ask is an inline error with Retry (rule 19).
 */
@Component({
  selector: 'pd-storage-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButton, MatProgressBar, MatTooltip, PdPathText, PdTable, PdCell, PdMiddleText, PdGlyph],
  template: `
    <section class="pd-storage__block" aria-labelledby="pd-storage-title" data-testid="storage-page">
      <h2 id="pd-storage-title">{{ t.title }}</h2>
      <p class="pd-storage__muted">{{ t.lede }}</p>
    </section>

    @if (store.working(); as job) {
      <div class="pd-storage__notice" role="status" data-testid="storage-working">
        <span class="pd-storage__title">{{ store.progress()?.label ?? (job === 'clear' ? t.clearing : t.measuring) }}</span>
        @if (store.progress(); as p) {
          <mat-progress-bar [attr.aria-label]="p.label" [mode]="p.total === null ? 'indeterminate' : 'determinate'" [value]="percent(p)" />
          <span class="pd-storage__muted" data-testid="storage-count">{{ p.total === null ? p.done : p.done + ' / ' + p.total }}</span>
        } @else {
          <mat-progress-bar mode="indeterminate" [attr.aria-label]="t.working" />
        }
        <span class="pd-storage__muted" data-testid="storage-still-working">{{ t.stillWorking }}</span>
        <span class="pd-storage__row">
          <button matButton="outlined" type="button" data-testid="storage-cancel" [disabled]="store.cancelling()" (click)="store.cancel()">{{ store.cancelling() ? t.cancelling : t.cancel }}</button>
        </span>
      </div>
    }

    @if (store.failure(); as f) {
      <div class="pd-storage__notice pd-storage__notice--danger" role="alert" data-testid="storage-failure">
        <span class="pd-storage__title">{{ t.failed }}</span>
        <span>{{ f.message }}</span>
        @if (f.hint) {
          <span class="pd-storage__muted">{{ f.hint }}</span>
        }
        <span class="pd-storage__row">
          @if (f.retryable) {
            <button matButton="outlined" type="button" data-testid="storage-retry" (click)="store.scan()">{{ t.retry }}</button>
          }
          <button matButton="outlined" type="button" (click)="store.dismissFailure()">{{ t.dismiss }}</button>
        </span>
      </div>
    }

    @if (store.result(); as r) {
      <div class="pd-storage__notice pd-storage__notice--success" role="status" data-testid="storage-result">
        <span class="pd-storage__title">{{ r.cancelled ? stopped(r.freedBytes) : freed(r.freedBytes) }}</span>
        @for (c of r.categories; track c.id) {
          @if (c.error) {
            <span data-testid="storage-result-error">{{ label(c.id) }}: {{ c.error }}</span>
          }
          @if (c.skippedCount > 0) {
            <details data-testid="storage-skipped">
              <summary>{{ label(c.id) }}: {{ skippedText(c.skippedCount) }}</summary>
              <ul>
                @for (s of c.skipped; track s.path) {
                  <li><pd-path-text [value]="s.path" /> <span class="pd-storage__muted">{{ s.reason }}</span></li>
                }
              </ul>
            </details>
          }
        }
        <span class="pd-storage__row"><button matButton="outlined" type="button" (click)="store.dismissResult()">{{ t.dismiss }}</button></span>
      </div>
    }

    @if (store.usage(); as usage) {
      <div class="pd-storage__summary">
        <p data-testid="storage-total">{{ totalText(usage.totalBytes, usage.capped) }}</p>
        <span class="pd-storage__row">
          <button matButton="outlined" type="button" data-testid="storage-clear-cache" [disabled]="!!store.working() || store.cache().ids.length === 0" (click)="clearCache()">{{ cacheLabel() }}</button>
          <button matButton="outlined" type="button" data-testid="storage-measure" [disabled]="!!store.working()" (click)="store.scan()">{{ t.measure }}</button>
        </span>
      </div>
      <pd-table
        data-testid="storage-table"
        [label]="t.tableLabel"
        [rows]="usage.categories"
        [columns]="categoryColumns"
        [rowKey]="categoryKey"
        [groupBy]="'kind'"
        [groupSummary]="categorySummary"
        [countLabel]="categoriesCount"
        [rowActions]="categoryActions"
        [filterable]="false"
        [maxHeight]="640"
        (action)="onCategoryAction($event)"
      >
        <ng-template pdCell="label" let-c>
          <span class="pd-storage__name" [matTooltip]="c.note || ''">
            <span class="pd-storage__title">{{ c.label }}</span>
            @if (c.note) {
              <span class="pd-storage__note">{{ c.note }}</span>
            }
          </span>
        </ng-template>
        <ng-template pdCell="size" let-c><span [matTooltip]="exact(c)" data-testid="storage-size">{{ sizeText(c) }}</span></ng-template>
      </pd-table>
      @if (openTree(); as id) {
        @if (store.trees()[id]; as tree) {
          <section class="pd-storage__tree" [attr.aria-label]="treeTitle(label(id))" data-testid="storage-tree">
            <div class="pd-storage__tree-head">
              <h3>{{ treeTitle(label(id)) }}</h3>
              <button type="button" class="pd-storage__link" data-testid="storage-tree-close" (click)="openTree.set(null)">{{ t.close }}</button>
            </div>
            @if (tree.capped) {
              <p class="pd-storage__muted" data-testid="storage-tree-capped">{{ t.treeCapped }}</p>
            }
            <pd-table
              [label]="treeTitle(label(id))"
              [rows]="treeData().rows"
              [columns]="treeColumns"
              [rowKey]="treeKey"
              [groupBy]="'group'"
              [groupSummary]="treeSummary"
              [groupMono]="treeGroupIsName"
              [countLabel]="itemsCount"
              [filterable]="false"
              [maxHeight]="360"
            >
              <ng-template pdCell="name" let-r>
                <span class="pd-storage__item">
                  @if (treeIcon(r); as icon) {
                    <pd-glyph [name]="icon" [size]="16" />
                  } @else {
                    <span class="pd-storage__nokind" aria-hidden="true"></span>
                  }
                  <pd-middle-text [value]="r.name" />
                </span>
              </ng-template>
            </pd-table>
          </section>
        }
      }
    } @else if (store.requestError(); as problem) {
      <div class="pd-storage__notice pd-storage__notice--danger" role="alert" data-testid="storage-error">
        <span class="pd-storage__title">{{ t.noUsage }}</span>
        <span>{{ problem }}</span>
        <span class="pd-storage__row"><button matButton="outlined" type="button" data-testid="storage-load-retry" (click)="store.load()">{{ t.retry }}</button></span>
      </div>
    } @else if (!store.working()) {
      <p class="pd-storage__muted" role="status" data-testid="storage-loading">{{ t.loading }}</p>
    }
  `,
  styles: STORAGE_STYLES,
})
export class PdStoragePage {
  protected readonly store = inject(StorageStore);
  private readonly dialogs = inject(DialogService);

  protected readonly t = {
    title: $localize`:@@pd.storage.title:Storage`,
    lede: $localize`:@@pd.storage.lede:What this app keeps on your computer. Nothing is removed unless you ask, and only inside the app's own folders.`,
    loading: $localize`:@@pd.storage.loading:Asking what the app keeps…`,
    noUsage: $localize`:@@pd.storage.no-usage-title:What the app keeps could not be read.`,
    retry: $localize`:@@pd.storage.retry:Retry`,
    dismiss: $localize`:@@pd.storage.dismiss:Dismiss`,
    failed: $localize`:@@pd.storage.failed:That did not work.`,
    measure: $localize`:@@pd.storage.measure:Measure again`,
    measuring: $localize`:@@pd.storage.measuring:Measuring what the app keeps…`,
    clearing: $localize`:@@pd.storage.clearing:Clearing…`,
    working: $localize`:@@pd.storage.working:Working…`,
    stillWorking: $localize`:@@pd.storage.still-working:Still working. This can take a while for big folders; nothing here gives up on it.`,
    cancel: $localize`:@@pd.storage.cancel:Cancel`,
    cancelling: $localize`:@@pd.storage.cancelling:Cancelling…`,
    tableLabel: $localize`:@@pd.storage.table-label:What the app keeps`,
    colName: $localize`:@@pd.storage.col-name:Kept`,
    colFolder: $localize`:@@pd.storage.col-folder:Folder`,
    colSize: $localize`:@@pd.storage.col-size:Size`,
    colActions: $localize`:@@pd.storage.col-actions:Actions`,
    cache: $localize`:@@pd.storage.kind-cache:Cache`,
    data: $localize`:@@pd.storage.kind-data:Your data`,
    show: $localize`:@@pd.storage.show:Show in the file manager`,
    folders: $localize`:@@pd.storage.folders:Folders`,
    close: $localize`:@@pd.storage.tree-close:Close`,
    inside: $localize`:@@pd.storage.inside:What is inside`,
    groupCache: $localize`:@@pd.storage.group-cache:Cache: rebuilt by the app`,
    groupData: $localize`:@@pd.storage.group-data:Your data: cannot be rebuilt`,
    atLeast: $localize`:@@pd.storage.at-least:At least (the walk stopped early):`,
    treeFiles: $localize`:@@pd.storage.tree-other:Other items`,
    treeCapped: $localize`:@@pd.storage.tree-capped:The walk stopped early (too many items or too long), so these sizes are at least that much.`,
  };

  protected readonly openTree = signal<string | null>(null);
  protected readonly treeData = computed(() => {
    const id = this.openTree();
    const tree = id ? this.store.trees()[id] : undefined;
    return tree
      ? treeTable(tree, { files: this.t.treeFiles, others: (n) => this.othersText(n) })
      : { rows: [] as readonly TreeRow[], groups: {} as Readonly<Record<string, { bytes: number; share: number }>> };
  });
  protected readonly treeKey = (row: TreeRow): string => row.key;
  protected readonly treeColumns: readonly PdColumn<TreeRow>[] = [
    { key: 'group', header: $localize`:@@pd.storage.col-group:Folder`, value: (r) => r.group, hidden: true },
    { key: 'name', header: $localize`:@@pd.storage.col-item:Item`, value: (r) => r.name, required: true, width: 'minmax(0, 3fr)' },
    { key: 'bytes', header: $localize`:@@pd.storage.col-size:Size`, value: (r) => r.bytes, text: (r) => formatStorage(r.bytes), align: 'end', width: 'minmax(64px, 96px)' },
    { key: 'share', header: $localize`:@@pd.storage.col-share:Share`, value: (r) => r.share, text: (r) => `${r.share}%`, align: 'end', width: '72px', hideBelow: 420 },
  ];
  /** The group heading's own size and share: the folder's, so its rows add up to it. */
  protected readonly treeSummary = (rows: readonly TreeRow[]): Readonly<Record<string, string>> => {
    const g = this.treeData().groups[rows[0]?.group ?? ''];
    return g ? { bytes: formatStorage(g.bytes), share: `${g.share}%` } : {};
  };
  protected readonly itemsCount = (shown: number, total: number): string =>
    shown === total ? $localize`:@@pd.storage.items:${total}:total: items` : $localize`:@@pd.storage.items-of:${shown}:shown: of ${total}:total: items`;

  /** Real folder names are set in mono; the "Other items" heading is a label, not a name. */
  protected readonly treeGroupIsName = (group: string): boolean => group !== this.t.treeFiles;

  protected treeIcon(r: TreeRow): string | null {
    return r.kind === 'folder' ? 'folder' : r.kind === 'fold' ? null : fileIcon(r.name);
  }

  // The categories: one shared pd-table, one line per row, grouped by what they are.
  protected readonly categoriesCount = (shown: number): string => $localize`:@@pd.storage.categories:${shown}:count: kinds of data`;
  protected readonly categoryKey = (c: StorageCategory): string => c.id;
  protected readonly categoryColumns: readonly PdColumn<StorageCategory>[] = [
    { key: 'kind', header: $localize`:@@pd.storage.col-kind:Kind`, value: (c) => c.kind, text: (c) => (c.kind === 'cache' ? this.t.groupCache : this.t.groupData), hidden: true },
    { key: 'label', header: $localize`:@@pd.storage.col-name:Kept`, value: (c) => c.label, required: true, width: 'minmax(0, 2fr)' },
    { key: 'path', header: $localize`:@@pd.storage.col-folder:Folder`, value: (c) => c.path ?? '', truncate: 'middle', width: 'minmax(0, 2fr)', hideBelow: 640 },
    { key: 'size', header: $localize`:@@pd.storage.col-size:Size`, value: (c) => c.bytes, align: 'end', width: 'minmax(64px, 96px)', required: true },
  ];
  protected readonly categorySummary = (rows: readonly StorageCategory[]): Readonly<Record<string, string>> => {
    const bytes = rows.reduce((n, c) => n + c.bytes, 0);
    const capped = rows.some((c) => c.capped);
    return { size: capped ? `≥ ${floorBytes(bytes)}` : formatStorage(bytes) };
  };
  protected readonly categoryActions = (c: StorageCategory): readonly PdRowAction[] => {
    const busy = !!this.store.working();
    const actions: PdRowAction[] = [];
    if (c.exists) {
      actions.push({ id: 'tree', label: this.t.inside, icon: 'layers', inline: true, disabled: busy });
      actions.push({ id: 'reveal', label: this.t.show, icon: 'reveal', inline: true });
    }
    if (c.clearable && c.clearableItems > 0) actions.push({ id: 'clear', label: this.clearLabel(c), danger: true, disabled: busy });
    return actions;
  };

  protected onCategoryAction(e: PdRowActionEvent<StorageCategory>): void {
    if (e.id === 'tree') void this.toggleTree(e.row);
    else if (e.id === 'reveal') void this.store.reveal(e.row.id);
    else if (e.id === 'clear') void this.clearCategory(e.row);
  }

  /** Shows (or hides) the size tree of a category; the engine walks the folder as a job. */
  protected async toggleTree(c: StorageCategory): Promise<void> {
    if (this.openTree() === c.id) {
      this.openTree.set(null);
      return;
    }
    this.openTree.set(c.id);
    if (!this.store.trees()[c.id]) await this.store.tree(c.id);
  }

  protected treeTitle(name: string): string {
    return $localize`:@@pd.storage.tree-title:What is in ${name}:name:`;
  }

  protected othersText(n: number): string {
    return n === 1 ? $localize`:@@pd.storage.other-one:1 smaller item` : $localize`:@@pd.storage.other-many:${n}:count: smaller items`;
  }

  protected label(id: string): string {
    return this.store.categories().find((c) => c.id === id)?.label ?? id;
  }

  /** The bar's fill in percent; only called when the total is known (otherwise the bar is indeterminate). */
  protected percent(p: { done: number; total: number | null }): number {
    return p.total ? Math.min(100, Math.max(0, (p.done / p.total) * 100)) : 0;
  }

  protected sizeText(c: StorageCategory): string {
    return c.capped ? `≥ ${floorBytes(c.bytes)}` : formatStorage(c.bytes);
  }

  protected exact(c: StorageCategory): string {
    const files = c.files;
    const base = $localize`:@@pd.storage.exact:${files}:files: files, ${formatStorage(c.clearableBytes)}:clearable: can be cleared`;
    return c.capped ? `${this.t.atLeast} ${base}` : base;
  }

  protected totalText(bytes: number, capped: boolean): string {
    const size = capped ? floorBytes(bytes) : formatStorage(bytes);
    return capped ? $localize`:@@pd.storage.total-least:The app keeps at least ${size}:size: on this computer.` : $localize`:@@pd.storage.total:The app keeps ${size}:size: on this computer.`;
  }

  protected cacheLabel(): string {
    const bytes = this.store.cache().bytes;
    return bytes > 0 ? $localize`:@@pd.storage.clear-cache-size:Clear cache (${formatStorage(bytes)}:size:)…` : $localize`:@@pd.storage.clear-cache:Clear cache`;
  }

  protected clearLabel(c: StorageCategory): string {
    const name = inSentence(c.label);
    return $localize`:@@pd.storage.clear-label:Clear ${name}:name:`;
  }

  protected freed(bytes: number): string {
    return $localize`:@@pd.storage.freed:Cleared. ${formatStorage(bytes)}:size: freed.`;
  }

  protected stopped(bytes: number): string {
    return $localize`:@@pd.storage.stopped:Stopped. ${formatStorage(bytes)}:size: freed so far; the rest was left as it was.`;
  }

  protected skippedText(n: number): string {
    return n === 1 ? $localize`:@@pd.storage.skipped-one:1 item left in place` : $localize`:@@pd.storage.skipped-many:${n}:count: items left in place`;
  }

  /** Asks first (a modal window), then clears the cache categories; the cache is rebuilt by the app, so it needs no per-category confirmation. */
  protected async clearCache(): Promise<void> {
    const { ids, bytes } = this.store.cache();
    if (ids.length === 0) return;
    const what = ids.length === 1 ? this.label(ids[0]!) : $localize`:@@pd.storage.the-cache:the cache`;
    const ok = answerOf(await this.dialogs.open<boolean>(STORAGE_CONFIRM_DIALOG_ID, { inputs: { what, bytes, data: false } }).closed);
    if (ok === true) await this.store.clear(ids.map((id) => ({ id, confirmed: false })));
  }

  /** Asks first, naming what goes (and that it is the user's own data when it is), then clears this category. */
  protected async clearCategory(c: StorageCategory): Promise<void> {
    const data = c.kind === 'data';
    const ok = answerOf(await this.dialogs.open<boolean>(STORAGE_CONFIRM_DIALOG_ID, { inputs: { what: c.label, bytes: c.clearableBytes, data } }).closed);
    if (ok === true) await this.store.clear([{ id: c.id, confirmed: data }]);
  }
}
