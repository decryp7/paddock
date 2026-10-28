import { TestBed } from '@angular/core/testing';
import { DIALOG_REF } from '@decryp7/paddock-ui/shell/dialogs';
import { appProviders, makeMock, text } from '../../../testing/test-helpers';
import { StopProcessDialog, confirmStopProcess } from './stop-process-dialog';

describe('StopProcessDialog', () => {
  function mount(inputs: { name: string; pid: number; isRunner: boolean; jobRunning: boolean }) {
    const close = vi.fn();
    TestBed.configureTestingModule({ providers: [...appProviders(makeMock()), { provide: DIALOG_REF, useValue: { close } }] });
    const f = TestBed.createComponent(StopProcessDialog);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return { el: f.nativeElement as HTMLElement, close };
  }
  const button = (el: HTMLElement, label: string) => Array.from(el.querySelectorAll('button')).find((b) => text(b) === label)!;

  it('names the process and says how it is stopped; Stop now is the default key', () => {
    const { el, close } = mount({ name: 'sleep.exe', pid: 7, isRunner: false, jobRunning: false });
    expect(text(el)).toContain('Stop sleep.exe?');
    expect(text(el)).toContain('asked to stop first');
    expect(text(el)).toContain('Only processes the runner started');
    expect(button(el, 'Stop now').hasAttribute('data-default')).toBe(true);
    button(el, 'Stop now').click();
    expect(close).toHaveBeenCalledWith(true);
  });

  it('warns more strongly for the runner process, and that a running job will fail', () => {
    const { el, close } = mount({ name: 'gitea-runner.exe', pid: 9, isRunner: true, jobRunning: true });
    expect(text(el)).toContain('Stop the runner process?');
    expect(text(el)).toContain('Pitcrew will restart it');
    expect(text(el)).toContain('The job that is running now will fail.');
    button(el, 'Cancel').click();
    expect(close).toHaveBeenCalledWith();
  });

  it('opens as a window of its own; only a confirmed choice is true', async () => {
    const openDialog = vi.fn().mockResolvedValueOnce(true).mockResolvedValueOnce(undefined);
    const data = { name: 'a', pid: 1, isRunner: false, jobRunning: false };
    expect(await confirmStopProcess({ openDialog }, data)).toBe(true);
    expect(openDialog).toHaveBeenCalledWith('pc.stop-process', data);
    expect(await confirmStopProcess({ openDialog }, data)).toBe(false);
  });
});
