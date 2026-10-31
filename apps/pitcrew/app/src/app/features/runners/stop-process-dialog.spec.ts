import { TestBed } from '@angular/core/testing';
import { answered, DISMISSED } from '@decryp7/paddock-ui/shell';
import { appProviders, makeMock, text } from '../../../testing/test-helpers';
import { StopProcessDialog, confirmStopProcess } from './stop-process-dialog';

describe('StopProcessDialog', () => {
  function mount(inputs: { name: string; pid: number; isRunner: boolean; jobRunning: boolean }) {
    TestBed.configureTestingModule({ providers: [...appProviders(makeMock())] });
    const f = TestBed.createComponent(StopProcessDialog);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return { el: f.nativeElement as HTMLElement, c: f.componentInstance };
  }
  const signal = new AbortController().signal;

  it('names the process and says how it is stopped; Stop now is the destructive answer', async () => {
    const { el, c } = mount({ name: 'sleep.exe', pid: 7, isRunner: false, jobRunning: false });
    expect(c.dialogHeading()).toBe('Stop sleep.exe?');
    expect(text(el)).toContain('asked to stop first');
    expect(text(el)).toContain('Only processes the runner started');
    const stop = c.dialogActions().find((a) => a.id === 'stop')!;
    expect(stop).toMatchObject({ kind: 'answer', label: 'Stop now', role: 'destructive' });
    expect(stop.kind === 'answer' && (await stop.answer(signal))).toBe(true);
  });

  it('warns more strongly for the runner process, and that a running job will fail', () => {
    const { el, c } = mount({ name: 'gitea-runner.exe', pid: 9, isRunner: true, jobRunning: true });
    expect(c.dialogHeading()).toBe('Stop the runner process?');
    expect(text(el)).toContain('Pitcrew will restart it');
    expect(text(el)).toContain('The job that is running now will fail.');
    expect(c.dialogActions().map((a) => a.label)).toEqual(['Cancel', 'End the runner process']);
    expect(c.dialogActions()[0]!.kind).toBe('dismiss');
  });

  it('opens as a window of its own; only a confirmed choice is true', async () => {
    const openDialog = vi.fn().mockResolvedValueOnce(answered(true)).mockResolvedValueOnce(DISMISSED);
    const data = { name: 'a', pid: 1, isRunner: false, jobRunning: false };
    expect(await confirmStopProcess({ openDialog }, data)).toBe(true);
    expect(openDialog).toHaveBeenCalledWith('pc.stop-process', data);
    expect(await confirmStopProcess({ openDialog }, data)).toBe(false);
  });
});
