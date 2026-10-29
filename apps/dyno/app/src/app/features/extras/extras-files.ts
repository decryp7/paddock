import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { PdCell, PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { FoundFile } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatDate, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { JobProgressCard } from '../../shared/job-progress';
import { ExtrasStore } from '../../state/extras.store';
import { MIN_SIZES, MODES, baseName, dirName, fileRows, middleEllipsis, scanSentence } from './files-model';
import { EXTRAS_STYLES } from './extras-styles';
import { MatButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';

interface FileLine {
  readonly key: string;
  readonly file: FoundFile;
  readonly group: string;
  readonly detail: string;
}

/**
 * Large and duplicate files in a folder the person chooses. Searching only reads: Dyno lists the files and moves or
 * removes none of them.
 */
@Component({
  selector: 'dy-extras-files',
  imports: [AsyncState, Icon, PdTable, PdCell, MatTooltip, SuSegmentedControl, MatButton, JobProgressCard],
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
      @if (store.finderJob(); as job) {
        <dy-job-progress [progress]="job" (cancel)="store.cancelSearch()" />
      }
      @if (store.finderCancelled()) {
        <p class="note faint" role="status">
          Search stopped. @if (result()) { The earlier result is shown. }
          <button type="button" matButton="text" (click)="store.search()">Search again</button>
        </p>
      }
      @if (result() || (!store.finderJob() && !store.finderCancelled())) {
      <dy-async [loaded]="!!result()" [error]="store.finder().error" what="the folder" [rows]="6" (retry)="store.search()">
        @if (result(); as r) {
          @if (fileList().length === 0) {
            <div class="none">
              <dy-icon name="check" />
              <strong>Nothing found in this folder</strong>
              <span>No file is at least {{ minLabel() }} and no files with identical content were found. {{ scanned() }}</span>
            </div>
          } @else {
            <pd-table [toolbar]="false" label="Large and duplicate files" [rows]="fileList()" [columns]="columns" [rowKey]="fileKey" [filterable]="false" [maxHeight]="620" groupBy="group" [groupSummary]="groupSummary">
              <ng-template pdCell="file" let-f><span class="fcell"><strong class="fname">{{ base(f.file.path) }}</strong><span class="dir path">{{ short(dir(f.file.path), 56) }}</span></span></ng-template>
            </pd-table>
            <p class="faint muted">{{ scanned() }} @if (r.duplicates.length > 0) { Copies are listed oldest first. }</p>
          }
        }
      </dy-async>
      }
    }
  `,
  styles: [
    EXTRAS_STYLES,
    `
      .folder { max-width: 360px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-2); }
      .fcell { display: flex; align-items: baseline; gap: var(--sp-2); min-width: 0; }
      .fname { font-weight: var(--fw-medium); flex: none; }
      .dir { color: var(--text-3); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
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

  protected readonly result = computed(() => this.store.finder().data);
  /** The files, each with the group it belongs to ("Large files", or a set of copies); the model builds the groups, the table folds them. */
  protected readonly fileList = computed<FileLine[]>(() => {
    const r = this.result();
    const out: FileLine[] = [];
    let group: { title: string; detail: string } | null = null;
    for (const row of r ? fileRows(r, new Set()) : []) {
      if (row.kind === 'group') group = { title: row.title, detail: row.detail };
      else if (group) out.push({ key: row.key, file: row.file, group: group.title, detail: group.detail });
    }
    return out;
  });
  protected readonly fileKey = (f: FileLine): string => f.key;
  protected readonly groupSummary = (rows: readonly FileLine[]): Record<string, string> => ({ size: rows[0]?.detail ?? '' });
  protected readonly columns: PdColumn<FileLine>[] = [
    { key: 'group', header: 'Group', value: (f) => f.group, hidden: true, sortable: false },
    { key: 'file', header: 'File', value: (f) => f.file.path, required: true, sortable: false, width: 'minmax(0, 1fr)' },
    { key: 'size', header: 'Size', value: (f) => f.file.bytes, text: (f) => formatBytes(f.file.bytes), align: 'end', sortable: false, width: '230px' },
    { key: 'modified', header: 'Modified', value: (f) => f.file.modified, text: (f) => formatDate(f.file.modified, Date.now()), align: 'end', sortable: false, width: '110px', hideBelow: 560 },
  ];
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
}
