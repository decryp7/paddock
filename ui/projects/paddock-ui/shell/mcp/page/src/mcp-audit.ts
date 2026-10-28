import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { AuditRow, Outcome } from '@decryp7/paddock-ui/shell/mcp';

/** One line of the viewer: the engine's row and a key that is unique within the list. */
interface Line {
  readonly key: string;
  readonly row: AuditRow;
}

const OUTCOME_WORDS: Readonly<Record<Outcome, string>> = {
  ok: $localize`:@@pd.mcp.outcome-ok:Answered`,
  'tool-error': $localize`:@@pd.mcp.outcome-tool-error:Failed`,
  refused: $localize`:@@pd.mcp.outcome-refused:Refused`,
  'timed-out': $localize`:@@pd.mcp.outcome-timed-out:Timed out`,
  panicked: $localize`:@@pd.mcp.outcome-panicked:Crashed`,
  cancelled: $localize`:@@pd.mcp.outcome-cancelled:Cancelled by the client`,
};

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * The shared audit viewer (ADR 0007 rule 3): every call a client made, newest first, as the family's table (`pd-table`): time,
 * client, tool, outcome, size, duration, and the arguments exactly as the engine redacted them (long values shorten in the
 * middle; the full text is the tooltip). Clear empties the log; a failed read is the table's inline error with Retry. It holds
 * no state of its own: the page's store owns the rows.
 */
@Component({
  selector: 'pd-mcp-audit',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButton, PdTable],
  template: `
    <div class="pd-au__head">
      <h3 id="pd-au-title">{{ t.title }}</h3>
      <button matButton="outlined" type="button" data-testid="mcp-audit-clear" [disabled]="rows().length === 0 || state() !== 'ready'" (click)="clear.emit()">{{ t.clear }}</button>
    </div>
    <pd-table
      [rows]="lines()"
      [columns]="columns"
      [rowKey]="rowKey"
      [label]="t.title"
      [state]="state()"
      [errorText]="errorText()"
      [emptyText]="t.empty"
      [filterable]="false"
      [maxHeight]="360"
      (retry)="retry.emit()"
      data-testid="mcp-audit"
    />
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; container-type: inline-size; }
    h3 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
    .pd-au__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--pd-space-2); }
  `,
})
export class PdMcpAudit {
  /** The calls, newest first. */
  readonly rows = input.required<readonly AuditRow[]>();
  /** `loading` shows skeleton rows (never timed out), `error` the message with Retry. */
  readonly state = input<'ready' | 'loading' | 'error'>('ready');
  /** What the error says. */
  readonly errorText = input('');
  /** Clear was pressed. */
  readonly clear = output<void>();
  /** Retry was pressed in the error state. */
  readonly retry = output<void>();

  protected readonly t = {
    title: $localize`:@@pd.mcp.audit-title:Activity`,
    clear: $localize`:@@pd.mcp.audit-clear:Clear`,
    empty: $localize`:@@pd.mcp.audit-empty:No client has called a tool yet.`,
  };

  protected readonly lines = computed<readonly Line[]>(() => this.rows().map((row, i) => ({ key: `${i}:${row.atMs}:${row.tool}`, row })));
  protected readonly rowKey = (l: Line): string => l.key;

  protected readonly columns: readonly PdColumn<Line>[] = [
    { key: 'time', header: $localize`:@@pd.mcp.col-time:Time`, value: (l) => l.row.atMs, text: (l) => new Date(l.row.atMs).toLocaleString(), width: '11rem', required: true },
    { key: 'client', header: $localize`:@@pd.mcp.col-client:Client`, value: (l) => l.row.client, width: 'minmax(5rem, 1fr)' },
    { key: 'tool', header: $localize`:@@pd.mcp.col-tool:Tool`, value: (l) => l.row.tool, width: 'minmax(6rem, 1.4fr)', truncate: 'middle', required: true },
    { key: 'outcome', header: $localize`:@@pd.mcp.col-outcome:Result`, value: (l) => OUTCOME_WORDS[l.row.outcome], width: '8rem', hideBelow: 420 },
    { key: 'size', header: $localize`:@@pd.mcp.col-size:Size`, value: (l) => l.row.resultBytes, text: (l) => size(l.row.resultBytes), align: 'end', width: '6rem', hideBelow: 560 },
    { key: 'duration', header: $localize`:@@pd.mcp.col-duration:Took (ms)`, value: (l) => l.row.durationMs, align: 'end', width: '6rem', hideBelow: 640 },
    { key: 'args', header: $localize`:@@pd.mcp.col-args:Arguments (redacted)`, value: (l) => JSON.stringify(l.row.arguments ?? null), width: 'minmax(8rem, 2fr)', truncate: 'middle', hideBelow: 760 },
  ];
}
