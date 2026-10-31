import type { ShellPorts } from '@decryp7/paddock-ui/shell';
import { confirmAction, type ConfirmData } from '../../shared/confirm';

export interface StopProcessData {
  name: string;
  pid: number;
  /** The runner process itself: a stronger warning, and the only way to set `allowRunner`. */
  isRunner: boolean;
  /** A job is running now (ending the runner process fails it). */
  jobRunning: boolean;
}

/** The words of stopping one process of a runner's tree, as the confirmation's data (the body, then the details). */
export function stopProcessData(data: StopProcessData): ConfirmData {
  if (data.isRunner) {
    return {
      title: 'Stop the runner process?',
      body: `This ends the runner process ${data.name} (pid ${data.pid}); Pitcrew will restart it.${data.jobRunning ? ' The job that is running now will fail.' : ''}`,
      details: ['It is asked to stop first and ended after the stop timeout if it does not.'],
      confirm: 'End the runner process',
      danger: true,
    };
  }
  return {
    title: `Stop ${data.name}?`,
    body: `${data.name} (pid ${data.pid}) is asked to stop first, and ended after the stop timeout if it ignores the request.`,
    details: ['Only processes the runner started can be stopped. Work the process has not saved is lost.'],
    confirm: 'Stop now',
    danger: true,
  };
}

/** Asks in the shared `pd.confirm` (a window of its own, modal); resolves true when the user confirmed. */
export function confirmStopProcess(ports: Pick<ShellPorts, 'openDialog'>, data: StopProcessData): Promise<boolean> {
  return confirmAction(ports, stopProcessData(data));
}
