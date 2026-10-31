import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { answerOf, type ShellPorts } from '@decryp7/paddock-ui/shell';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

export interface StopProcessData {
  name: string;
  pid: number;
  /** The runner process itself: a stronger warning, and the only way to set `allowRunner`. */
  isRunner: boolean;
  /** A job is running now (ending the runner process fails it). */
  jobRunning: boolean;
}

/** Confirms stopping one process of a runner's tree. The frame draws the question, the icon and the buttons. */
@Component({
  selector: 'pc-stop-process-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (isRunner()) {
      <p>
        This ends the runner process <strong>{{ name() }}</strong> (pid {{ pid() }}); Pitcrew will restart it.
        @if (jobRunning()) {
          <strong class="tone-danger"> The job that is running now will fail.</strong>
        }
      </p>
      <p class="muted">It is asked to stop first and ended after the stop timeout if it does not.</p>
    } @else {
      <p>
        <strong>{{ name() }}</strong> (pid {{ pid() }}) is asked to stop first, and ended after the stop timeout if it ignores the request.
      </p>
      <p class="muted">Only processes the runner started can be stopped. Work the process has not saved is lost.</p>
    }
  `,
  styles: `
    p {
      margin: 0 0 var(--space-3);
    }
  `,
})
export class StopProcessDialog implements DialogContent<boolean> {
  readonly name = input.required<string>();
  readonly pid = input.required<number>();
  readonly isRunner = input(false);
  readonly jobRunning = input(false);

  readonly dialogHeading = computed(() => (this.isRunner() ? 'Stop the runner process?' : 'Stop ' + this.name() + '?'));
  readonly dialogActions = computed<readonly DialogAction<boolean>[]>(() => [
    { kind: 'dismiss', id: 'cancel', label: 'Cancel' },
    { kind: 'answer', id: 'stop', label: this.isRunner() ? 'End the runner process' : 'Stop now', role: 'destructive', answer: () => true },
  ]);
}

/** The dialog's id (`pc.stop-process` in the contributions). */
export const STOP_PROCESS_DIALOG_ID = 'pc.stop-process';

/** Opens the confirmation as its own window (modal); resolves true when the user confirmed. */
export async function confirmStopProcess(ports: Pick<ShellPorts, 'openDialog'>, data: StopProcessData): Promise<boolean> {
  return answerOf(await ports.openDialog(STOP_PROCESS_DIALOG_ID, data)) === true;
}
