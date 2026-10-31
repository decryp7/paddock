import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { answerOf, type ShellPorts } from '@decryp7/paddock-ui/shell';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import type { ConfigPreview } from '../../../core/api.types';
import { diffSymbol, diffWord } from './config-issues';
import { FieldIssues } from './field-issues';

export interface ConfigDiffData {
  preview: ConfigPreview;
  /** Backups kept next to the file. */
  maxBackups: number;
}

/** Shows what saving would change (the engine's diff) and what happens next; Cancel or Save and apply. */
@Component({
  selector: 'pc-config-diff-dialog',
  imports: [MatIcon, FieldIssues],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="wrap">
      <div class="scroll">
      <p class="keys">
        <span class="counts" aria-label="Lines added and removed">
          <span class="added"><span aria-hidden="true">+</span>{{ data().preview.diff.added }} <span class="visually-hidden">lines added</span></span>
          <span class="removed"><span aria-hidden="true">−</span>{{ data().preview.diff.removed }} <span class="visually-hidden">lines removed</span></span>
        </span>
        @if (data().preview.changedKeys.length > 0) {
          Changes: <code>{{ data().preview.changedKeys.join(', ') }}</code>
        }
      </p>
      <div class="diff" role="region" aria-label="Changes to config.yaml" tabindex="0">
        @for (h of data().preview.diff.hunks; track $index) {
          @if ($index > 0) {
            <div class="gap" aria-hidden="true">⋯</div>
          }
          <table>
            <caption class="visually-hidden">Changed lines {{ $index + 1 }}</caption>
            <tbody>
              @for (l of h.lines; track $index) {
                <tr [class]="l.kind">
                  <td class="no" aria-hidden="true">{{ l.oldNo ?? '' }}</td>
                  <td class="no" aria-hidden="true">{{ l.newNo ?? '' }}</td>
                  <td class="sym"><span aria-hidden="true">{{ sym(l.kind) }}</span><span class="visually-hidden">{{ word(l.kind) }}:</span></td>
                  <td class="txt">{{ l.text }}</td>
                </tr>
              }
            </tbody>
          </table>
        }
        @if (data().preview.diff.truncated) {
          <p class="gap">The list of changes is cut off.</p>
        }
        @if (data().preview.diff.hunks.length === 0) {
          <p class="gap">No lines changed.</p>
        }
      </div>

      <pc-field-issues [issues]="data().preview.issues" prefix="diff-issue" />

      <ul class="notes">
        <li><mat-icon svgIcon="pc:folder" aria-hidden="true" />Saves a backup of the current file (config.yaml.bak-…; the last {{ data().maxBackups }} are kept).</li>
        @if (data().preview.restartNeeded) {
          <li><mat-icon svgIcon="pc:refresh" aria-hidden="true" />The runner restarts after its current job to use these settings.</li>
        } @else {
          <li><mat-icon svgIcon="pc:check" aria-hidden="true" />Only comments or layout change; the runner does not need to restart.</li>
        }
      </ul>
      </div>
    </div>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      min-height: 0;
    }
    .wrap {
      display: flex;
      flex: 1;
      flex-direction: column;
      gap: var(--space-3);
      min-height: 0;
      min-width: 0;
    }
    .scroll {
      display: grid;
      flex: 1;
      align-content: start;
      gap: var(--space-3);
      min-height: 0;
      min-width: 0;
      overflow: auto;
    }
    .counts {
      display: inline-flex;
      gap: var(--space-3);
      font-family: var(--font-mono);
      font-weight: var(--weight-semibold);
    }
    .added {
      color: var(--success);
    }
    .removed {
      color: var(--danger);
    }
    .keys {
      margin: 0;
      color: var(--text-2);
      font-size: var(--text-sm);
    }
    .diff {
      max-height: 320px;
      min-width: 0;
      overflow: auto;
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      background: var(--surface-2);
      font-family: var(--font-mono);
      font-size: var(--text-sm);
    }
    table {
      width: 100%;
      border-collapse: collapse;
    }
    td {
      padding: 0 var(--space-2);
      white-space: pre;
      line-height: 1.6;
    }
    .no {
      width: 1%;
      text-align: right;
      color: var(--text-3);
      user-select: none;
    }
    .sym {
      width: 1%;
      font-weight: var(--weight-semibold);
      text-align: center;
    }
    tr.added {
      background: var(--success-soft);
    }
    tr.added .sym {
      color: var(--success);
    }
    tr.removed {
      background: var(--danger-soft);
    }
    tr.removed .sym {
      color: var(--danger);
    }
    tr.removed .txt {
      text-decoration: line-through;
      text-decoration-thickness: 1px;
    }
    .gap {
      margin: 0;
      padding: 2px var(--space-3);
      color: var(--text-3);
      background: var(--surface-3);
    }
    .notes {
      display: grid;
      gap: var(--space-1);
      margin: 0;
      padding: 0;
      list-style: none;
      color: var(--text-2);
    }
    .notes li {
      display: flex;
      gap: var(--space-2);
      align-items: flex-start;
    }
    .notes mat-icon {
      flex: none;
      width: 16px;
      height: 16px;
      margin-top: 2px;
    }
  `,
})
export class ConfigDiffDialog implements DialogContent<boolean> {
  readonly preview = input.required<ConfigPreview>();
  readonly maxBackups = input.required<number>();
  protected readonly data = computed<ConfigDiffData>(() => ({ preview: this.preview(), maxBackups: this.maxBackups() }));
  /** Cancel, and Save and apply: the frame draws the footer. */
  readonly dialogActions = computed<readonly DialogAction<boolean>[]>(() => [
    { kind: 'dismiss', id: 'cancel', label: 'Cancel' },
    { kind: 'answer', id: 'save', label: 'Save and apply', role: 'primary', answer: () => true },
  ]);
  protected readonly sym = diffSymbol;
  protected readonly word = diffWord;
}

/** The dialog's id (`pc.config-diff` in the contributions). */
export const CONFIG_DIFF_DIALOG_ID = 'pc.config-diff';

/** Opens the review as its own window (modal); resolves true when the user chose Save and apply. */
export async function reviewConfigChanges(ports: Pick<ShellPorts, 'openDialog'>, data: ConfigDiffData): Promise<boolean> {
  return answerOf(await ports.openDialog(CONFIG_DIFF_DIALOG_ID, data)) === true;
}
