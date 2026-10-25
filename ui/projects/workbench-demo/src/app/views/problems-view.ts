import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { PdPathText } from '@decryp7/paddock-ui';
import { statusGlyph } from '@decryp7/paddock-ui/icons';
import { PdIcon, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { PdCell, PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { WorkbenchDemoContext } from '../context';
import { DemoRepo, type Problem } from '../repo';

const SEVERITY: Readonly<Record<Problem['level'], number>> = { error: 0, warning: 1 };

/**
 * The problems list (an analysis view) on the shared `pd-table` (X21). Severity is a 14 px glyph with its word as the
 * accessible name and tooltip (never colour alone); the columns drop by priority as the pane narrows (Problem, then File,
 * then Line) and below 360 px the file and line fold under the problem as a muted second line. Sorted by severity, then
 * file. A row opens its file.
 */
@Component({
  selector: 'wd-problems-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdTable, PdCell, PdIcon, PdPathText],
  template: `
    <pd-table label="Problems" [rows]="rows" [columns]="columns" [rowKey]="key" [maxHeight]="2000" (rowActivate)="open($event)">
      <ng-template pdCell="level" let-r>
        <span class="sev" [attr.data-level]="r.level" role="img" [attr.aria-label]="word(r)" [attr.title]="word(r)">
          <pd-icon [name]="'pd:' + glyph(r.level)" />
        </span>
      </ng-template>
      <ng-template pdCell="message" let-r let-folded="folded">
        <span class="msg">
          <span class="text" [attr.title]="r.message">{{ r.message }}</span>
          @if (folded) {
            <span class="where"><pd-path-text [value]="r.path + ':' + r.line" [plain]="true" /></span>
          }
        </span>
      </ng-template>
    </pd-table>
  `,
  styles: `
    :host { display: block; height: 100%; overflow: auto; padding: var(--pd-space-2); container: wd-problems / inline-size; }
    .sev { display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 16px; }
    .sev[data-level='error'] { color: var(--pd-danger); }
    .sev[data-level='warning'] { color: var(--pd-warning); }
    .msg { display: flex; flex-direction: column; min-width: 0; }
    .text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    /* Below 360 px File and Line are gone: where the problem is folds under it (the table grows the rows: foldBelow). */
    .where { display: flex; min-width: 0; color: var(--pd-text-muted); font-size: var(--pd-text-label); line-height: 1.3; }
    .text { line-height: 1.4; }
  `,
})
export class ProblemsView {
  /** The shared shape of a severity (error circle-×, warning triangle, info circle-i). */
  protected readonly glyph = (level: Problem['level']) => statusGlyph(level);
  private readonly host = inject(ShellContextHost);
  protected readonly rows = [...inject(DemoRepo).problems].sort((a, b) => SEVERITY[a.level] - SEVERITY[b.level] || a.path.localeCompare(b.path) || a.line - b.line);
  protected readonly key = (p: Problem) => `${p.path}:${p.line}`;
  protected readonly columns: PdColumn<Problem>[] = [
    { key: 'level', header: 'Severity', value: (p) => SEVERITY[p.level], text: (p) => p.level, width: '16px', required: true, headerHidden: true, sortable: false },
    { key: 'message', header: 'Problem', value: (p) => p.message, required: true, width: 'minmax(0, 3fr)', foldBelow: 360 },
    { key: 'path', header: 'File', value: (p) => p.path, truncate: 'middle', width: 'minmax(0, 2fr)', hideBelow: 360 },
    { key: 'line', header: 'Line', value: (p) => p.line, align: 'end', width: '56px', hideBelow: 440 },
  ];

  protected word(p: Problem): string {
    return p.level === 'error' ? 'Error' : 'Warning';
  }

  protected open(p: Problem): void {
    void (this.host.context() as unknown as WorkbenchDemoContext | null)?.openFile(p.path);
  }
}
