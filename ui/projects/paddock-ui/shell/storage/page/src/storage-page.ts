import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { PdPathText } from '@decryp7/paddock-ui';
import { formatBytes } from '@decryp7/paddock-ui/format';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { STORAGE_CONFIRM_DIALOG_ID, StorageStore, type StorageCategory } from '@decryp7/paddock-ui/shell/storage';
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
  imports: [PdPathText],
  template: `
    <section class="pd-storage__block" aria-labelledby="pd-storage-title" data-testid="storage-page">
      <h2 id="pd-storage-title">{{ t.title }}</h2>
      <p class="pd-storage__muted">{{ t.lede }}</p>
    </section>

    @if (store.working(); as job) {
      <div class="pd-storage__notice" role="status" data-testid="storage-working">
        <span class="pd-storage__title">{{ store.progress()?.label ?? (job === 'clear' ? t.clearing : t.measuring) }}</span>
        @if (store.progress(); as p) {
          <progress [attr.aria-label]="p.label" [attr.value]="p.total === null ? null : p.done" [attr.max]="p.total"></progress>
          <span class="pd-storage__muted" data-testid="storage-count">{{ p.total === null ? p.done : p.done + ' / ' + p.total }}</span>
        } @else {
          <progress [attr.aria-label]="t.working"></progress>
        }
        <span class="pd-storage__muted" data-testid="storage-still-working">{{ t.stillWorking }}</span>
        <span class="pd-storage__row">
          <button type="button" class="pd-storage__btn" data-testid="storage-cancel" [disabled]="store.cancelling()" (click)="store.cancel()">{{ store.cancelling() ? t.cancelling : t.cancel }}</button>
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
            <button type="button" class="pd-storage__btn" data-testid="storage-retry" (click)="store.scan()">{{ t.retry }}</button>
          }
          <button type="button" class="pd-storage__btn" (click)="store.dismissFailure()">{{ t.dismiss }}</button>
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
        <span class="pd-storage__row"><button type="button" class="pd-storage__btn" (click)="store.dismissResult()">{{ t.dismiss }}</button></span>
      </div>
    }

    @if (store.usage(); as usage) {
      <div class="pd-storage__summary">
        <p data-testid="storage-total">{{ totalText(usage.totalBytes, usage.capped) }}</p>
        <span class="pd-storage__row">
          <button type="button" class="pd-storage__btn" [class.pd-storage__btn--primary]="store.cache().ids.length > 0" data-testid="storage-clear-cache" [disabled]="!!store.working() || store.cache().ids.length === 0" (click)="clearCache()">{{ cacheLabel() }}</button>
          <button type="button" class="pd-storage__btn" data-testid="storage-measure" [disabled]="!!store.working()" (click)="store.scan()">{{ t.measure }}</button>
        </span>
      </div>
      <table class="pd-storage__table" data-testid="storage-table">
        <caption class="pd-storage__sr">{{ t.tableLabel }}</caption>
        <thead>
          <tr>
            <th scope="col">{{ t.colName }}</th>
            <th scope="col" class="pd-storage__folder">{{ t.colFolder }}</th>
            <th scope="col" class="pd-storage__size">{{ t.colSize }}</th>
            <th scope="col"><span class="pd-storage__sr">{{ t.colActions }}</span></th>
          </tr>
        </thead>
        <tbody>
          @for (c of usage.categories; track c.id) {
            <tr [attr.data-testid]="'storage-row-' + c.id">
              <td class="pd-storage__name">
                <span class="pd-storage__title">{{ c.label }}</span>
                <span class="pd-storage__kind">{{ c.kind === 'cache' ? t.cache : t.data }}</span>
                @if (c.note) {
                  <span class="pd-storage__muted pd-storage__note">{{ c.note }}</span>
                }
              </td>
              <td class="pd-storage__folder">
                @if (c.path) {
                  <pd-path-text [value]="c.path" />
                }
              </td>
              <td class="pd-storage__size" [attr.title]="exact(c)">{{ sizeText(c) }}</td>
              <td class="pd-storage__actions">
                @if (c.exists) {
                  <button type="button" class="pd-storage__btn" [attr.data-testid]="'storage-show-' + c.id" [attr.aria-label]="showLabel(c)" (click)="store.reveal(c.id)">{{ t.show }}</button>
                }
                @if (c.kind === 'data' && c.clearable && c.clearableItems > 0) {
                  <button type="button" class="pd-storage__btn" [attr.data-testid]="'storage-clear-' + c.id" [attr.aria-label]="clearLabel(c)" [disabled]="!!store.working()" (click)="clearData(c)">{{ t.clear }}</button>
                }
              </td>
            </tr>
          }
        </tbody>
      </table>
    } @else if (store.requestError(); as problem) {
      <div class="pd-storage__notice pd-storage__notice--danger" role="alert" data-testid="storage-error">
        <span class="pd-storage__title">{{ t.noUsage }}</span>
        <span>{{ problem }}</span>
        <span class="pd-storage__row"><button type="button" class="pd-storage__btn" data-testid="storage-load-retry" (click)="store.load()">{{ t.retry }}</button></span>
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
    show: $localize`:@@pd.storage.show:Show`,
    clear: $localize`:@@pd.storage.clear:Clear…`,
  };

  protected label(id: string): string {
    return this.store.categories().find((c) => c.id === id)?.label ?? id;
  }

  protected sizeText(c: StorageCategory): string {
    return (c.capped ? '≥ ' : '') + formatBytes(c.bytes);
  }

  protected exact(c: StorageCategory): string {
    const files = c.files;
    return $localize`:@@pd.storage.exact:${files}:files: files, ${formatBytes(c.clearableBytes)}:clearable: can be cleared`;
  }

  protected totalText(bytes: number, capped: boolean): string {
    const size = (capped ? '≥ ' : '') + formatBytes(bytes);
    return $localize`:@@pd.storage.total:The app keeps ${size}:size: on this computer.`;
  }

  protected cacheLabel(): string {
    const bytes = this.store.cache().bytes;
    return bytes > 0 ? $localize`:@@pd.storage.clear-cache-size:Clear cache (${formatBytes(bytes)}:size:)…` : $localize`:@@pd.storage.clear-cache:Clear cache`;
  }

  protected showLabel(c: StorageCategory): string {
    return $localize`:@@pd.storage.show-label:Show ${c.label}:name: in the file manager`;
  }

  protected clearLabel(c: StorageCategory): string {
    return $localize`:@@pd.storage.clear-label:Clear ${c.label}:name:`;
  }

  protected freed(bytes: number): string {
    return $localize`:@@pd.storage.freed:Cleared. ${formatBytes(bytes)}:size: freed.`;
  }

  protected stopped(bytes: number): string {
    return $localize`:@@pd.storage.stopped:Stopped. ${formatBytes(bytes)}:size: freed so far; the rest was left as it was.`;
  }

  protected skippedText(n: number): string {
    return n === 1 ? $localize`:@@pd.storage.skipped-one:1 item left in place` : $localize`:@@pd.storage.skipped-many:${n}:count: items left in place`;
  }

  /** Asks first (a modal window), then clears the cache categories; the cache is rebuilt by the app, so it needs no per-category confirmation. */
  protected async clearCache(): Promise<void> {
    const { ids, bytes } = this.store.cache();
    if (ids.length === 0) return;
    const what = ids.length === 1 ? this.label(ids[0]!) : $localize`:@@pd.storage.the-cache:the cache`;
    const ok = await this.dialogs.open<boolean>(STORAGE_CONFIRM_DIALOG_ID, { inputs: { what, bytes, data: false } }).closed;
    if (ok === true) await this.store.clear(ids.map((id) => ({ id, confirmed: false })));
  }

  /** Asks first, naming the data, then clears this category with the user's confirmation. */
  protected async clearData(c: StorageCategory): Promise<void> {
    const ok = await this.dialogs.open<boolean>(STORAGE_CONFIRM_DIALOG_ID, { inputs: { what: c.label, bytes: c.clearableBytes, data: true } }).closed;
    if (ok === true) await this.store.clear([{ id: c.id, confirmed: true }]);
  }
}
