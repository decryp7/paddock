import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../core/mock-backend';
import { JobsStore } from '../state/jobs.store';
import { appProviders, makeMock } from '../../testing/test-helpers';
import { OPERATIONS_TIMING, Operations } from './operations';

async function setup(options = {}, tickMs = 5) {
  const backend: MockBackend = makeMock(options);
  TestBed.configureTestingModule({ providers: [...appProviders(backend), { provide: OPERATIONS_TIMING, useValue: { showAfterMs: 0, quietAfterMs: 10_000, tickMs } }] });
  const fixture = TestBed.createComponent(Operations);
  fixture.detectChanges();
  return { backend, jobs: TestBed.inject(JobsStore), fixture, el: fixture.nativeElement as HTMLElement };
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe('Operations panel', () => {
  afterEach(() => vi.useRealTimers());

  it('shows nothing when nothing is running', async () => {
    const { el } = await setup();
    expect(el.querySelector('.op')).toBeNull();
  });

  it('shows a download with its phase, bytes of the total and a Cancel button', async () => {
    const { backend, jobs, fixture, el } = await setup({ jobStepMs: 30 });
    const version = (await backend.getWatch('w-1')).history[0]!.version;
    const run = jobs.run('download', 'Downloading Lumen CLI 2.4.1', async (id) => backend.downloadRelease('w-1', version, id));
    await wait(120);
    fixture.detectChanges();
    const row = el.querySelector('.op') as HTMLElement;
    expect(row.textContent).toContain('Downloading Lumen CLI 2.4.1');
    expect(row.querySelector('.phase')?.textContent).toContain('Downloading');
    expect(row.querySelector('.numbers')?.textContent).toMatch(/MiB of 176 MiB/);
    const bar = row.querySelector('progress') as HTMLProgressElement;
    expect(bar.max).toBe(1);
    expect(bar.value).toBeGreaterThan(0);
    (row.querySelector('button') as HTMLButtonElement).click();
    await expect(run).rejects.toMatchObject({ code: 'cancelled' });
    fixture.detectChanges();
    expect(el.querySelector('.op')).toBeNull();
  });

  it('labels each phase of the browser install separately, each on its own scale', async () => {
    const { backend, jobs, fixture, el } = await setup({ jobStepMs: 15 });
    const labels = new Set<string>();
    const run = jobs.run('installRenderer', 'Installing the browser', (id) => backend.installRenderer(id));
    for (let i = 0; i < 40; i++) {
      await wait(10);
      fixture.detectChanges();
      const phase = el.querySelector('.phase')?.textContent?.trim();
      if (phase) labels.add(phase.replace(/\s+/g, ' '));
    }
    await run;
    expect([...labels].some((l) => l.startsWith('Downloading'))).toBe(true);
    expect([...labels].some((l) => l.startsWith('Unpacking'))).toBe(true);
    expect([...labels].some((l) => l.startsWith('Testing'))).toBe(true);
  });

  it('shows an indeterminate bar and a growing count when the total is not known', async () => {
    const { backend, jobs, fixture, el } = await setup({ jobStepMs: 40 });
    const draft = { ...(await backend.getWatch('w-1')).spec };
    draft.source = { kind: 'folder', values: { path: '/share' } };
    const run = jobs.run('testSource', 'Testing the source', (id) => backend.testSource(draft, {}, id));
    await wait(100);
    fixture.detectChanges();
    expect(el.querySelector('.numbers')?.textContent).toContain('found');
    expect((el.querySelector('progress') as HTMLProgressElement).hasAttribute('value')).toBe(false);
    await run;
  });

  it('says a job is still working when the engine has been quiet for a while, and never gives up on it', async () => {
    vi.useFakeTimers();
    const { jobs, fixture, el } = await setup({}, 1_000);
    // Quick jobs are not shown; this one has been running a while.
    let release: () => void = () => undefined;
    const run = jobs.run('download', 'Downloading a big file', () => new Promise<void>((r) => (release = r)));
    await vi.advanceTimersByTimeAsync(1_000);
    fixture.detectChanges();
    expect(el.querySelector('.op')?.textContent).toContain('Starting');
    expect(el.textContent).not.toContain('Still working');
    await vi.advanceTimersByTimeAsync(20 * 60_000);
    // (the panel reads the clock on its own tick)
    fixture.detectChanges();
    expect(el.querySelector('.op')).not.toBeNull();
    expect(el.textContent).toContain('Still working');
    expect(el.textContent).not.toContain('did not answer');
    release();
    await run;
    fixture.detectChanges();
    expect(el.querySelector('.op')).toBeNull();
  });

  it('hides a job that ends before it is worth showing', async () => {
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend) });
    const fixture = TestBed.createComponent(Operations);
    fixture.detectChanges();
    await TestBed.inject(JobsStore).run('check', 'Checking', (id) => backend.checkNow('w-1', id));
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('.op')).toBeNull();
  });

  it('waits a moment before showing a job, so a quick one does not flash a panel', async () => {
    const backend = makeMock({ jobStepMs: 5 });
    TestBed.configureTestingModule({ providers: [...appProviders(backend), { provide: OPERATIONS_TIMING, useValue: { showAfterMs: 120, quietAfterMs: 10_000, tickMs: 10 } }] });
    const fixture = TestBed.createComponent(Operations);
    fixture.detectChanges();
    let release: () => void = () => undefined;
    const run = TestBed.inject(JobsStore).run('check', 'Checking', () => new Promise<void>((r) => (release = r)));
    await wait(40);
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('.op')).toBeNull();
    await wait(200);
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('.op')).not.toBeNull();
    release();
    await run;
  });
});
