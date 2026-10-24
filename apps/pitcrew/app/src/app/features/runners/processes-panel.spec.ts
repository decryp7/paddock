import { Overlay } from '@angular/cdk/overlay';
import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { NOW, NoticeLog, appProviders, byText, flush, makeMock, settle, text } from '../../../testing/test-helpers';
import { ProcessesPanel } from './processes-panel';

describe('ProcessesPanel', () => {
  let mock: MockBackend;
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(NOW);
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
  });
  afterEach(() => {
    vi.useRealTimers();
    document.querySelectorAll('.cdk-overlay-container > *').forEach((n) => n.remove());
    mock.dispose();
  });

  async function mount(name = 'build-win') {
    await mock.status();
    const f = TestBed.createComponent(ProcessesPanel);
    f.componentRef.setInput('runner', name);
    await settle(f, 6);
    return { f, el: f.nativeElement as HTMLElement };
  }
  const row = (el: HTMLElement, name: string) => Array.from(el.querySelectorAll('tbody tr')).find((r) => text(r.querySelector('.name-line')).startsWith(name)) as HTMLElement;
  const log = () => TestBed.inject(NoticeLog);
  /** What the confirmation window was last opened with (its words are tested with the dialog itself). */
  const asked = () => log().dialogs.at(-1)?.inputs as { name: string; pid: number; isRunner: boolean; jobRunning: boolean };

  it('summarises the processes, the cleanup state and whether a job runs', async () => {
    const { el } = await mount();
    expect(text(el.querySelector('[data-testid="process-summary"]'))).toBe('8 processes, 3 leftovers');
    expect(text(el.querySelector('[data-testid="cleanup-state"]'))).toBe('Automatic cleanup: on, keeps leftovers 10 min after a job');
    expect(text(el)).toContain('A job is running');
    expect(el.querySelector('table.procs')!.querySelector('caption')!.textContent).toContain('Processes of build-win');
  });

  it('marks leftovers with a word, counts down to the stop and names the rule', async () => {
    const { el } = await mount();
    const gradle = row(el, 'java.exe');
    expect(text(gradle)).toContain('Leftover');
    expect(text(gradle.querySelector('[data-testid="fate"]'))).toBe('stops in 2 h 14 min · kept by rule *GradleDaemon*');
    expect(text(row(el, 'sleep.exe').querySelector('[data-testid="fate"]'))).toBe('stops in 7 min · default 10 min');
    expect(text(row(el, 'adb.exe').querySelector('[data-testid="fate"]'))).toBe('never stopped (rule adb) · kept by rule adb');
    expect(gradle.querySelector('.cmd')!.getAttribute('title')).toContain('GradleDaemon');
    expect(text(row(el, 'MSBuild.exe'))).toContain('running');
    expect(text(row(el, 'MSBuild.exe'))).toContain('acme/storefront');
  });

  it('shows the runner row with a badge and no plain Stop button', async () => {
    const { el } = await mount();
    const r = row(el, 'gitea-runner.exe');
    expect(text(r)).toContain('runner');
    expect(r.querySelector('button[aria-label^="Stop"]')).toBeNull();
    expect(r.querySelector('button[aria-label^="More actions"]')).not.toBeNull();
    expect(row(el, 'node.exe').querySelector('button[aria-label^="Stop node.exe now"]')).not.toBeNull();
  });

  it('only offers Keep longer for leftovers that will be stopped', async () => {
    const { el } = await mount();
    expect(row(el, 'java.exe').querySelector('button[aria-label="Keep java.exe longer"]')).not.toBeNull();
    expect(row(el, 'adb.exe').querySelector('button[aria-label^="Keep"]')).toBeNull();
    expect(row(el, 'node.exe').querySelector('button[aria-label^="Keep"]')).toBeNull();
  });

  it('keeps a leftover longer from the menu', async () => {
    const { f, el } = await mount();
    (row(el, 'sleep.exe').querySelector('button[aria-label="Keep sleep.exe longer"]') as HTMLElement).click();
    await settle(f, 2);
    const items = Array.from(document.querySelectorAll('[cdkMenuItem]')).map((x) => text(x));
    expect(items).toEqual(['+ 30 minutes', '+ 1 hour', '+ 3 hours', '+ 1 day']);
    (document.querySelectorAll('[cdkMenuItem]')[1] as HTMLElement).click();
    await settle(f, 4);
    expect(text(row(el, 'sleep.exe').querySelector('[data-testid="fate"]'))).toContain('stops in 1 h 07 min');
  });

  it('asks before stopping, names the process, and removes the row', async () => {
    const { f, el } = await mount();
    (row(el, 'sleep.exe').querySelector('button[aria-label^="Stop sleep.exe"]') as HTMLElement).click();
    await settle(f, 2);
    expect(log().dialogs.at(-1)?.id).toBe('pc.stop-process');
    expect(asked()).toMatchObject({ name: 'sleep.exe', isRunner: false });
    // Dismissed: the row stays.
    expect(row(el, 'sleep.exe')).toBeDefined();
    log().answerNext(true);
    (row(el, 'sleep.exe').querySelector('button[aria-label^="Stop sleep.exe"]') as HTMLElement).click();
    await settle(f, 8);
    expect(row(el, 'sleep.exe')).toBeUndefined();
    expect(text(el.querySelector('[data-testid="process-summary"]'))).toContain('2 leftovers');
  });

  it('needs an explicit, stronger confirmation to end the runner process', async () => {
    const { f, el } = await mount('ui-tests');
    const spy = vi.spyOn(mock, 'stopProcess');
    (row(el, 'gitea-runner.exe').querySelector('button[aria-label^="More actions"]') as HTMLElement).click();
    await settle(f, 2);
    log().answerNext(true);
    (document.querySelector('[cdkMenuItem]') as HTMLElement).click();
    await settle(f, 6);
    expect(asked().isRunner).toBe(true);
    expect(spy.mock.calls[0]![3]).toBe(true);
  });

  it('warns that a running job will fail when the runner process is ended', async () => {
    const { f, el } = await mount();
    (row(el, 'gitea-runner.exe').querySelector('button[aria-label^="More actions"]') as HTMLElement).click();
    await settle(f, 2);
    (document.querySelector('[cdkMenuItem]') as HTMLElement).click();
    await settle(f, 2);
    expect(asked()).toMatchObject({ isRunner: true, jobRunning: true });
  });

  it('shows the engine’s text when a stop is refused', async () => {
    const { f, el } = await mount();
    vi.spyOn(mock, 'stopProcess').mockRejectedValueOnce({ code: 'badRequest', message: 'that process is not part of this runner’s process tree (gone)' });
    log().answerNext(true);
    (row(el, 'node.exe').querySelector('button[aria-label^="Stop node.exe"]') as HTMLElement).click();
    await settle(f, 6);
    expect(text(el.querySelector('[role="alert"]'))).toContain('not part of this runner’s process tree');
    byText<HTMLButtonElement>(el, 'button', 'Dismiss')!.click();
    await settle(f, 2);
    expect(el.querySelector('[role="alert"]')).toBeNull();
  });

  it('copies a command', async () => {
    const { f, el } = await mount();
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    (row(el, 'sleep.exe').querySelector('button[aria-label^="Copy the command"]') as HTMLElement).click();
    await flush();
    expect(write).toHaveBeenCalledWith('sleep 3600');
    write.mockRejectedValueOnce(new Error('denied'));
    (row(el, 'sleep.exe').querySelector('button[aria-label^="Copy the command"]') as HTMLElement).click();
    await settle(f, 2);
  });

  it('says so when the runner is not running', async () => {
    const { el } = await mount('docker-lab');
    expect(text(el)).toContain('No processes: this runner is not running');
    expect(el.querySelector('table.procs')).toBeNull();
  });

  it('notes a cut-off list and an automatic cleanup that is off', async () => {
    vi.spyOn(mock, 'runnerProcesses').mockImplementation(async (r) => {
      const real = await MockBackend.prototype.runnerProcesses.call(mock, r);
      return { ...real, truncated: true, cleanup: false, idle: true };
    });
    const { el } = await mount();
    expect(text(el)).toContain('The list is cut off');
    expect(text(el.querySelector('[data-testid="cleanup-state"]'))).toBe('Automatic cleanup: off');
    expect(text(el)).toContain('Idle: leftovers can be stopped');
  });

  it('shows a load error with Retry', async () => {
    vi.spyOn(mock, 'runnerProcesses').mockRejectedValueOnce({ code: 'serviceUnavailable', message: 'down' });
    const { f, el } = await mount();
    expect(text(el)).toContain('could not be loaded');
    byText<HTMLButtonElement>(el, 'button', 'Retry')!.click();
    await settle(f, 6);
    expect(el.querySelector('table.procs')).not.toBeNull();
  });

  it('saves a runner’s overrides through the settings', async () => {
    const { f, el } = await mount();
    const select = el.querySelector('#runner-processes-cleanup') as HTMLSelectElement;
    select.value = 'off';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    f.detectChanges();
    byText<HTMLButtonElement>(el, 'button', 'Save overrides')!.click();
    await settle(f, 6);
    expect((await mock.settings()).config.runners[0]!.processes).toEqual({ cleanup: false });
    expect(text(el.querySelector('[data-testid="cleanup-state"]'))).toBe('Automatic cleanup: off');
    select.value = 'inherit';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    f.detectChanges();
    byText<HTMLButtonElement>(el, 'button', 'Save overrides')!.click();
    await settle(f, 6);
    expect((await mock.settings()).config.runners[0]!.processes).toBeNull();
  });

  it('is unused without Overlay leftovers', () => {
    expect(TestBed.inject(Overlay)).toBeDefined();
  });
});
