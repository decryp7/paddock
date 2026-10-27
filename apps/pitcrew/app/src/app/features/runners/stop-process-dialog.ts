import { DIALOG_DATA, Dialog, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { firstValueFrom } from 'rxjs';

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
        <span class="icon" [class.danger]="data.isRunner"><mat-icon [svgIcon]="data.isRunner ? 'pc:alert' : 'pc:stop-circle'" aria-hidden="true" /></span>
        <h2 id="stop-process-title">{{ data.isRunner ? 'Stop the runner process?' : 'Stop ' + data.name + '?' }}</h2>
      </div>
      @if (data.isRunner) {
        <p>
          This ends the runner process <strong>{{ data.name }}</strong> (pid {{ data.pid }}); Pitcrew will restart it.
          @if (data.jobRunning) {
            <strong class="tone-danger"> The job that is running now will fail.</strong>
          }
        </p>
        <p class="muted">It is asked to stop first and ended after the stop timeout if it does not.</p>
      } @else {
        <p>
          <strong>{{ data.name }}</strong> (pid {{ data.pid }}) is asked to stop first, and ended after the stop timeout if it ignores the request.
        </p>
        <p class="muted">Only processes the runner started can be stopped. Work the process has not saved is lost.</p>
      }
      <div class="btn-row end">
        <button type="button" class="btn" cdkFocusInitial (click)="ref.close(false)">Cancel</button>
        <button type="button" class="btn danger" (click)="ref.close(true)">{{ data.isRunner ? 'End the runner process' : 'Stop now' }}</button>
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
  protected readonly data = inject<StopProcessData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
}

/** Opens {@link StopProcessDialog}; resolves true when the user confirmed. */
export async function confirmStopProcess(dialog: Dialog, data: StopProcessData): Promise<boolean> {
  const ref = dialog.open<boolean>(StopProcessDialog, {
    data,
    ariaLabelledBy: 'stop-process-title',
    panelClass: 'pc-dialog-panel',
    backdropClass: 'pc-dialog-backdrop',
  });
  return (await firstValueFrom(ref.closed)) === true;
}
