import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { DeleteMode } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Dialogs } from '../../shared/dialog';
import { formatBytes, formatDate, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { ExtrasStore } from '../../state/extras.store';
import { CleanupConfirmDialog, type CleanupConfirmData } from '../cleanup/cleanup-confirm-dialog';
import { MIN_SIZES, MODES, type FileRow, baseName, dirName, fileRows, groupsEmptied, middleEllipsis, scanSentence, summariseFiles } from './files-model';
import { EXTRAS_STYLES } from './extras-styles';

/**
 * Large and duplicate files in a folder the person chooses. Searching only reads; files move to the Trash or the 7-day
 * quarantine through the cleanup policy after a confirmation that names the count and the bytes. Every copy of a duplicate
 * group except the oldest is selected by default, and the engine refuses to take the last one.
 */
@Component({
  selector: 'dy-extras-files',
  imports: [AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="bar" role="group" aria-label="Search">
      <button type="button" class="btn primary" [disabled]="store.finder().loading" (click)="store.chooseFolder()"><dy-icon name="folder" /> {{ store.folder() ? 'Choose another folder…' : 'Choose a folder…' }}</button>
      @if (store.folder(); as f) {
        <span class="folder path" [title]="f">{{ short(f) }}</span>
      }
      <span class="grow"></span>
      <div class="seg" role="group" aria-label="Look for">
        @for (m of modes; track m.mode) {
          <button type="button" [attr.aria-pressed]="store.mode() === m.mode" (click)="store.setMode(m.mode)">{{ m.label }}</button>
        }
      </div>
      <div class="seg" role="group" aria-label="At least this big">
        @for (s of sizes; track s.mib) {
          <button type="button" [attr.aria-pressed]="store.minMib() === s.mib" (click)="store.setMinMib(s.mib)">{{ s.label }}</button>
        }
      </div>
    </div>

    @if (!store.folder() && !store.finder().error) {
      <div class="none">
        <dy-icon name="folder" />
        <strong>Find big and duplicate files</strong>
        <span>Choose a folder and Dyno lists its biggest files and files with identical content. It only reads; nothing moves until you pick files and confirm.</span>
      </div>
    } @else {
      <dy-async [loaded]="!!result()" [error]="store.finder().error" what="the folder" [rows]="6" (retry)="store.search()">
        @if (result(); as r) {
          @if (rows().length === 0) {
            <div class="none">
              <dy-icon name="check" />
              <strong>Nothing found in this folder</strong>
              <span>No file is at least {{ minLabel() }} and no files with identical content were found. {{ scanned() }}</span>
            </div>
          } @else {
            <div class="selbar" role="group" aria-label="Selection">
              <span class="summary" aria-live="polite">{{ plural(sel().count, 'file') }} selected · {{ bytes(sel().bytes) }}</span>
              <button type="button" class="btn sm ghost" [disabled]="sel().count === 0" (click)="store.clearSelection()">Clear</button>
              @if (r.duplicates.length > 0) { <button type="button" class="btn sm ghost" (click)="store.resetSelection()">Reselect copies</button> }
              <span class="grow"></span>
              <button type="button" class="btn" [disabled]="!canRemove()" (click)="confirm('recycleBin')"><dy-icon name="trash" /> Move to Trash…</button>
              <button type="button" class="btn" [disabled]="!canRemove()" (click)="confirm('quarantine')"><dy-icon name="restore" /> Quarantine…</button>
            </div>
            @if (emptied().length > 0) {
              <p class="note bad" role="alert"><dy-icon name="alert" /><span>Keep at least one copy of each group: {{ plural(emptied().length, 'group') }} would lose every copy.</span></p>
            }
            @if (store.removeError(); as e) {
              <p class="note bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span></p>
            }
            <div class="tablewrap">
              <table class="table tbl files" aria-label="Large and duplicate files">
                <thead><tr><th class="pick"><span class="visually-hidden">Select</span></th><th>File</th><th class="r w-size">Size</th><th class="r w-mod">Modified</th></tr></thead>
                <tbody>
                  @for (row of rows(); track row.key) {
                    @if (row.kind === 'group') {
                      <tr class="day">
                        <td colspan="4">
                          <button type="button" class="chev" [attr.aria-expanded]="row.open" [attr.aria-label]="(row.open ? 'Collapse ' : 'Expand ') + row.title" (click)="toggleGroup(row.key)"><dy-icon [name]="row.open ? 'chevron-down' : 'chevron-right'" /></button>
                          <strong>{{ row.title }}</strong><span class="faint cnt">{{ row.detail }}</span>
                        </td>
                      </tr>
                    } @else {
                      <tr [class.on]="isSel(row)">
                        <td class="pick"><input type="checkbox" [checked]="isSel(row)" [attr.aria-label]="'Select ' + base(row.file.path)" (change)="store.toggleFile(row.file.id)" /></td>
                        <td>
                          <span class="ell" [title]="row.file.path"><strong class="fname">{{ base(row.file.path) }}</strong> <span class="dir path">{{ short(dir(row.file.path), 56) }}</span></span>
                        </td>
                        <td class="r">{{ bytes(row.file.bytes) }}</td>
                        <td class="r muted">{{ modified(row.file.modified) }}</td>
                      </tr>
                    }
                  }
                </tbody>
              </table>
            </div>
            <p class="faint muted">{{ scanned() }} @if (r.duplicates.length > 0) { Copies are listed oldest first. }</p>
          }
        }
      </dy-async>
    }
  `,
  styles: [
    EXTRAS_STYLES,
    `
      .folder { max-width: 360px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-2); }
      .selbar { display: flex; align-items: center; gap: var(--sp-2); flex-wrap: wrap; min-height: 40px; }
      .summary { font-weight: var(--fw-medium); margin-right: var(--sp-2); font-variant-numeric: tabular-nums; }
      .files th.pick, .files td.pick { width: 44px; text-align: center; padding: 0; }
      .w-size { width: 110px; }
      .w-mod { width: 110px; }
      .fname { font-weight: var(--fw-medium); }
      .dir { color: var(--text-3); margin-left: var(--sp-2); }
      tr.on td { background: var(--accent-soft); }
      .chev { display: inline-grid; place-items: center; width: 22px; height: 22px; border: 0; background: none; color: var(--text-3); border-radius: var(--r-sm); cursor: pointer; vertical-align: middle; }
      .chev:hover { background: var(--surface-active); color: var(--text-1); }
    `,
  ],
})
export class ExtrasFiles {
  protected readonly store = inject(ExtrasStore);
  private readonly dialogs = inject(Dialogs);
  private readonly notifier = inject(Notifier);
  protected readonly modes = MODES;
  protected readonly sizes = MIN_SIZES;
  private readonly collapsed = signal<ReadonlySet<string>>(new Set());

  protected readonly result = computed(() => this.store.finder().data);
  protected readonly selectedSet = computed(() => new Set(this.store.selected()));
  protected readonly rows = computed<FileRow[]>(() => {
    const r = this.result();
    return r ? fileRows(r, this.collapsed()) : [];
  });
  protected readonly sel = computed(() => summariseFiles(this.result(), this.selectedSet()));
  protected readonly emptied = computed(() => {
    const r = this.result();
    return r ? groupsEmptied(r, this.selectedSet()) : [];
  });
  protected readonly canRemove = computed(() => this.sel().count > 0 && this.emptied().length === 0 && !this.store.removing());
  protected readonly minLabel = computed(() => MIN_SIZES.find((s) => s.mib === this.store.minMib())?.label ?? formatBytes(this.store.minMib() * 1024 * 1024));
  protected readonly scanned = computed(() => {
    const r = this.result();
    return r ? scanSentence(r) : '';
  });

  protected readonly plural = plural;
  protected readonly bytes = formatBytes;
  protected readonly base = baseName;
  protected readonly dir = dirName;
  protected readonly short = (p: string, max = 48): string => middleEllipsis(p, max);
  protected readonly modified = (at: number): string => formatDate(at, Date.now());

  protected isSel(row: Extract<FileRow, { kind: 'file' }>): boolean {
    return this.selectedSet().has(row.file.id);
  }

  protected toggleGroup(key: string): void {
    this.collapsed.update((c) => {
      const next = new Set(c);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }

  /** The last question: names the count and the bytes, and lets the person change where the files go. */
  protected async confirm(mode: DeleteMode): Promise<void> {
    const s = this.sel();
    if (s.count === 0) return;
    const rows = [
      ...(s.copies.count ? [{ name: 'Duplicate copies', count: s.copies.count, bytes: s.copies.bytes }] : []),
      ...(s.large.count ? [{ name: 'Large files', count: s.large.count, bytes: s.large.bytes }] : []),
    ];
    const data: CleanupConfirmData = { count: s.count, bytes: s.bytes, rows, mode };
    const ref = this.dialogs.open<DeleteMode | null, CleanupConfirmData>(CleanupConfirmDialog, data);
    const chosen = await new Promise<DeleteMode | null>((resolve) => ref.closed.subscribe((m) => resolve(m ?? null)));
    if (!chosen) return;
    const outcome = await this.store.remove(chosen);
    if (outcome === null && !this.store.removeError()) this.notifier.show('Nothing was moved', 'warn');
  }
}
