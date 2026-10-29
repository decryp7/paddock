import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatDate, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { MIN_SIZES, MODES, type FileRow, baseName, dirName, fileRows, middleEllipsis, scanSentence } from './files-model';
import { EXTRAS_STYLES } from './extras-styles';
import { MatButton, MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';

/**
 * Large and duplicate files in a folder the person chooses. Searching only reads: Dyno lists the files and moves or
 * removes none of them.
 */
@Component({
  selector: 'dy-extras-files',
  imports: [MatIconButton, MatTooltip, SuSegmentedControl, MatButton, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="bar" role="group" aria-label="Search">
      <button type="button" matButton="filled" [disabled]="store.finder().loading" (click)="store.chooseFolder()"><dy-icon name="folder" /> {{ store.folder() ? 'Choose another folder…' : 'Choose a folder…' }}</button>
      @if (store.folder(); as f) {
        <span class="folder path" [matTooltip]="f">{{ short(f) }}</span>
      }
      <span class="grow"></span>
      <pd-segmented-control label="Look for" [options]="modeSegs" [value]="store.mode()" (valueChange)="pickMode($event)" />
      <pd-segmented-control label="At least this big" [options]="sizeSegs" [value]="store.minMib() + ''" (valueChange)="store.setMinMib(+$event)" />
    </div>

    @if (!store.folder() && !store.finder().error) {
      <div class="none">
        <dy-icon name="folder" />
        <strong>Find big and duplicate files</strong>
        <span>Choose a folder and Dyno lists its biggest files and files with identical content. It only reads and never moves or removes a file.</span>
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
            <div class="tablewrap">
              <table class="table tbl files" aria-label="Large and duplicate files">
                <thead><tr><th>File</th><th class="r w-size">Size</th><th class="r w-mod">Modified</th></tr></thead>
                <tbody>
                  @for (row of rows(); track row.key) {
                    @if (row.kind === 'group') {
                      <tr class="day">
                        <td colspan="3">
                          <button type="button" matIconButton [matTooltip]="(row.open ? 'Collapse ' : 'Expand ') + row.title" class="chev" [attr.aria-expanded]="row.open" [attr.aria-label]="(row.open ? 'Collapse ' : 'Expand ') + row.title" (click)="toggleGroup(row.key)"><dy-icon [name]="row.open ? 'chevron-down' : 'chevron-right'" /></button>
                          <strong>{{ row.title }}</strong><span class="faint cnt">{{ row.detail }}</span>
                        </td>
                      </tr>
                    } @else {
                      <tr>
                        <td>
                          <span class="ell" [matTooltip]="row.file.path"><strong class="fname">{{ base(row.file.path) }}</strong> <span class="dir path">{{ short(dir(row.file.path), 56) }}</span></span>
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
      .w-size { width: 110px; }
      .w-mod { width: 110px; }
      .fname { font-weight: var(--fw-medium); }
      .dir { color: var(--text-3); margin-left: var(--sp-2); }
    `,
  ],
})
export class ExtrasFiles {
  protected readonly store = inject(ExtrasStore);
  protected readonly modes = MODES;
  protected readonly sizes = MIN_SIZES;
  protected readonly modeSegs: SuSegment[] = MODES.map((m) => ({ value: m.mode, label: m.label }));
  protected readonly sizeSegs: SuSegment[] = MIN_SIZES.map((s) => ({ value: String(s.mib), label: s.label }));
  protected pickMode(value: string): void {
    const m = MODES.find((x) => x.mode === value);
    if (m) void this.store.setMode(m.mode);
  }
  private readonly collapsed = signal<ReadonlySet<string>>(new Set());

  protected readonly result = computed(() => this.store.finder().data);
  protected readonly rows = computed<FileRow[]>(() => {
    const r = this.result();
    return r ? fileRows(r, this.collapsed()) : [];
  });
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

  protected toggleGroup(key: string): void {
    this.collapsed.update((c) => {
      const next = new Set(c);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }
}
