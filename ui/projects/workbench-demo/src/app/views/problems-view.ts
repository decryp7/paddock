import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { ShellContextHost } from '@decryp7/paddock-ui/shell';
import { PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { WorkbenchDemoContext } from '../context';
import { DemoRepo, type Problem } from '../repo';

/** The problems list (an analysis view): the shared `pd-table` (X21); a row opens its file. */
@Component({
  selector: 'wd-problems-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdTable],
  template: `<pd-table label="Problems" [rows]="rows" [columns]="columns" [rowKey]="key" [maxHeight]="2000" (rowActivate)="open($event)" />`,
  styles: `:host { display: block; height: 100%; overflow: auto; padding: var(--pd-space-2); container: wd-problems / inline-size; }`,
})
export class ProblemsView {
  private readonly host = inject(ShellContextHost);
  protected readonly rows = inject(DemoRepo).problems;
  protected readonly key = (p: Problem) => `${p.path}:${p.line}`;
  protected readonly columns: PdColumn<Problem>[] = [
    { key: 'level', header: 'Level', value: (p) => p.level, width: '84px' },
    { key: 'message', header: 'Problem', value: (p) => p.message, required: true, width: 'minmax(0, 2fr)' },
    { key: 'path', header: 'File', value: (p) => p.path, truncate: 'middle', width: 'minmax(0, 1fr)', hideBelow: 420 },
    { key: 'line', header: 'Line', value: (p) => p.line, align: 'end', width: '56px', hideBelow: 520 },
  ];

  protected open(p: Problem): void {
    void (this.host.context() as unknown as WorkbenchDemoContext | null)?.openFile(p.path);
  }
}
