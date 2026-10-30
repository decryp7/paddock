import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { ShellPorts } from '@decryp7/paddock-ui/shell';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

export interface StopProcessData {
  name: string;
  pid: number;
  /** The runner process itself: a stronger warning, and the only way to set `allowRunner`. */
  isRunner: boolean;
  /** A job is running now (ending the runner process fails it). */
  jobRunning: boolean;
}

/** Confirms stopping one process of a runner's tree. */
@Component({
  selector: 'pc-stop-process-dialog',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="wrap">
      <div class="head">
        <span class="icon" [class.danger]="data().isRunner"><mat-icon [svgIcon]="data().isRunner ? 'pc:alert' : 'pc:stop-circle'" aria-hidden="true" /></span>
        <h2 id="stop-process-title">{{ data().isRunner ? 'Stop the runner process?' : 'Stop ' + data().name + '?' }}</h2>
      </div>
      @if (data().isRunner) {
        <p>
          This ends the runner process <strong>{{ data().name }}</strong> (pid {{ data().pid }}); Pitcrew will restart it.
          @if (data().jobRunning) {
            <strong class="tone-danger"> The job that is running now will fail.</strong>
          }
        </p>
        <p class="muted">It is asked to stop first and ended after the stop timeout if it does not.</p>
      } @else {
        <p>
          <strong>{{ data().name }}</strong> (pid {{ data().pid }}) is asked to stop first, and ended after the stop timeout if it ignores the request.
        </p>
        <p class="muted">Only processes the runner started can be stopped. Work the process has not saved is lost.</p>
      }
      <div class="btn-row end">
        <button type="button" class="btn" data-dialog-close (click)="ref.close()">Cancel</button>
        <button type="button" class="btn danger" data-default (click)="ref.close(true)">{{ data().isRunner ? 'End the runner process' : 'Stop now' }}</button>
      </div>
    </div>
  `,
  styles: `
    .wrap {
      display: grid;
      gap: var(--space-3);
      width: min(460px, calc(100vw - 48px));
      padding: var(--space-5);
    }
    p {
      margin: 0;
    }
    .head {
      display: flex;
      align-items: center;
      gap: var(--space-3);
    }
    .icon {
      display: grid;
      place-items: center;
      width: 32px;
      height: 32px;
      border-radius: var(--radius-full);
      background: var(--info-soft);
      color: var(--info);
    }
    .icon.danger {
      background: var(--danger-soft);
      color: var(--danger);
    }
    .end {
      justify-content: flex-end;
    }
  `,
})
export class StopProcessDialog {
  readonly name = input.required<string>();
  readonly pid = input.required<number>();
  readonly isRunner = input(false);
  readonly jobRunning = input(false);
  protected readonly data = computed<StopProcessData>(() => ({ name: this.name(), pid: this.pid(), isRunner: this.isRunner(), jobRunning: this.jobRunning() }));
  protected readonly ref = inject<DialogRef<boolean>>(DIALOG_REF);
}

/** The dialog's id (`pc.stop-process` in the contributions). */
export const STOP_PROCESS_DIALOG_ID = 'pc.stop-process';

/** Opens the confirmation as its own window (modal); resolves true when the user confirmed. */
export async function confirmStopProcess(ports: Pick<ShellPorts, 'openDialog'>, data: StopProcessData): Promise<boolean> {
  return (await ports.openDialog(STOP_PROCESS_DIALOG_ID, data)) === true;
}
