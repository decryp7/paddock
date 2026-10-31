import { answered, DISMISSED } from '@decryp7/paddock-ui/shell';
import { confirmStopProcess, stopProcessData } from './stop-process-confirm';

describe('stopping a process (the words on the shared pd.confirm)', () => {
  it('names the process and says how it is stopped; Stop now is destructive', () => {
    const d = stopProcessData({ name: 'sleep.exe', pid: 7, isRunner: false, jobRunning: false });
    expect(d).toMatchObject({ title: 'Stop sleep.exe?', confirm: 'Stop now', danger: true });
    expect(d.body).toContain('asked to stop first');
    expect(d.details?.[0]).toContain('Only processes the runner started');
  });

  it('warns more strongly for the runner process, and that a running job will fail', () => {
    const d = stopProcessData({ name: 'gitea-runner.exe', pid: 9, isRunner: true, jobRunning: true });
    expect(d).toMatchObject({ title: 'Stop the runner process?', confirm: 'End the runner process', danger: true });
    expect(d.body).toContain('Pitcrew will restart it');
    expect(d.body).toContain('The job that is running now will fail.');
    expect(stopProcessData({ name: 'g', pid: 9, isRunner: true, jobRunning: false }).body).not.toContain('will fail');
  });

  it('opens pd.confirm with the heading, message and details; only a confirmed choice is true', async () => {
    const openDialog = vi.fn().mockResolvedValueOnce(answered(true)).mockResolvedValueOnce(DISMISSED);
    const data = { name: 'a', pid: 1, isRunner: false, jobRunning: false };
    expect(await confirmStopProcess({ openDialog }, data)).toBe(true);
    expect(openDialog).toHaveBeenCalledWith('pd.confirm', expect.objectContaining({ heading: 'Stop a?', details: [expect.any(String)], confirm: { label: 'Stop now', destructive: true } }));
    expect(await confirmStopProcess({ openDialog }, data)).toBe(false);
  });
});
