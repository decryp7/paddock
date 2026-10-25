import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject } from '@angular/core';
import { errorHint } from '../../core/command-error';
import { DialogFrame } from '../../shared/dialog';
import { Icon } from '../../shared/icon';
import { WatchedStore } from '../../state/watched.store';
import { dueRow, dueSummary, folderName, keepLabel, staysText } from './watched-model';

/** What the preview dialog is opened with. */
export interface PreviewData {
  id: string;
}

/**
 * "What would move": a dry run of a sweep with every file named. This is what a person must see before a folder can be turned
 * on, and what "Clean now" acts on. Nothing is moved by looking.
 */
@Component({
  selector: 'dy-preview-dialog',
  imports: [DialogFrame, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="What would move" [subtitle]="subtitle()" icon="folder">
      @if (store.previewError(); as e) {
        <div class="err" role="alert"><dy-icon name="alert" /><span>{{ e.message }} @if (hint(); as h) {<small>{{ h }}</small>}</span></div>
      } @else if (store.preview(); as p) {
        <p class="lead">{{ summary() }}</p>
        @if (p.plan.due.length > 0) {
          <div class="list" tabindex="0" aria-label="Files that would move">
            <table class="table">
              <thead><tr><th>File</th><th class="r">Size</th><th class="r">Age</th></tr></thead>
              <tbody>
                @for (f of rows(); track f.path) {
                  <tr><td class="name"><span class="ell" [title]="f.path">{{ f.name }}</span></td><td class="r">{{ f.size }}</td><td class="r">{{ f.age }}</td></tr>
                }
              </tbody>
            </table>
          </div>
        }
        @if (stays(); as s) {
          <p class="stays">{{ s }}</p>
        }
        <p class="safe"><dy-icon name="shield-check" /> Files go to the 7-day quarantine and can be put back from Activity. Links are never followed and nothing outside this folder is touched.</p>
      } @else {
        <div class="skeleton box" aria-busy="true" aria-label="Looking at the folder"></div>
      }
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close()">Close</button>
        @if (can()) {
          <button type="button" class="btn" [class.primary]="folder()?.enabled" [disabled]="store.busy() !== null || due() === 0" (click)="clean()">Move {{ due() }} now</button>
          @if (!folder()?.enabled) {
            <button type="button" class="btn primary" [disabled]="store.busy() !== null" (click)="turnOn()">Turn on</button>
          }
        }
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    :host { --dialog-w: 600px; }
    .lead { margin: 0; color: var(--text-1); }
    .list { max-height: 260px; overflow: auto; border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-2); }
    .table { table-layout: fixed; }
    .table th.r, .table td.r { width: 90px; }
    .table td.name { overflow: hidden; }
    .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .stays { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .safe { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); }
    .safe dy-icon { --icon-size: 15px; color: var(--accent-text); flex: none; margin-top: 1px; }
    .box { height: 96px; }
    .err { display: flex; gap: var(--sp-2); color: var(--bad); }
    .err small { display: block; color: var(--text-2); }
  `,
})
export class PreviewDialog implements OnInit, OnDestroy {
  private readonly data = inject<PreviewData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
  protected readonly store = inject(WatchedStore);
  protected readonly folder = computed(() => this.store.overview()?.folders.find((f) => f.folder.id === this.data.id)?.folder ?? null);
  protected readonly subtitle = computed(() => {
    const f = this.folder();
    return f ? `${folderName(f.path)}: files older than ${keepLabel(f.keepForSecs)}` : '';
  });
  protected readonly summary = computed(() => {
    const p = this.store.preview();
    const f = this.folder();
    return p && f ? dueSummary(p.plan, f.keepForSecs) : '';
  });
  protected readonly stays = computed(() => {
    const p = this.store.preview();
    return p ? staysText(p.plan) : '';
  });
  protected readonly due = computed(() => this.store.preview()?.plan.dueTotal ?? 0);
  protected readonly can = computed(() => this.store.preview() !== null && this.folder() !== null);
  private readonly now = Date.now();
  protected readonly rows = computed(() => (this.store.preview()?.plan.due ?? []).map((f) => dueRow(f, this.now)));
  protected readonly hint = computed(() => {
    const e = this.store.previewError();
    return e ? errorHint(e.code) : null;
  });

  ngOnInit(): void {
    void this.store.loadPreview(this.data.id);
  }

  ngOnDestroy(): void {
    this.store.closePreview();
  }

  protected async turnOn(): Promise<void> {
    if (await this.store.change(this.data.id, { enabled: true }, 'Watching the folder. Old files move to the quarantine')) this.ref.close(true);
  }

  protected async clean(): Promise<void> {
    if (await this.store.sweep(this.data.id)) this.ref.close(true);
  }
}
