import { TestBed } from '@angular/core/testing';
import type { FinishedJob, MachineSample, RunnerSnapshot, ServiceInfo } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { StatusStore } from '../../state/status.store';
import { NOW, appProviders, byText, flush, makeMock, settle, text } from '../../../testing/test-helpers';
import { EventsFeed } from './events-feed';
import type { FeedItem } from './feed';
import { MachineTiles } from './machine-tiles';
import { RunnerTile } from './runner-tile';

describe('RunnerTile', () => {
  let mock: MockBackend;
  let runners: RunnerSnapshot[];

  beforeEach(async () => {
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    runners = (await mock.status()).runners;
  });
  afterEach(() => mock.dispose());

  async function mount(name: string, inputs: Record<string, unknown> = {}) {
    const f = TestBed.createComponent(RunnerTile);
    f.componentRef.setInput('runner', runners.find((r) => r.name === name));
    f.componentRef.setInput('now', NOW);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    await settle(f);
    return { f, el: f.nativeElement as HTMLElement };
  }

  it('shows the running job, its details and opens the run', async () => {
    const details = await mock.runDetails('build-win');
    const open = vi.spyOn(mock, 'openUrl').mockResolvedValue();
    const { el } = await mount('build-win', { details });
    expect(text(el.querySelector('.name'))).toBe('build-win');
    expect(text(el.querySelector('.repo'))).toContain('acme/storefront');
    expect(text(el.querySelector('.elapsed'))).toBe('07:32');
    expect(text(el.querySelector('.run'))).toContain('Retry declined card payments at checkout');
    expect(text(el.querySelector('.run'))).toContain('ci.yml');
    expect(text(el.querySelector('.run'))).toContain('feature/checkout-retry');
    (byText(el, 'button', 'Open run #342') as HTMLButtonElement).click();
    expect(open).toHaveBeenCalledWith(details!.url);
  });

  it('shows what the runner and its processes use, and nothing while it is not running', async () => {
    const { el } = await mount('build-win');
    expect(text(el.querySelector('.load'))).toBe('31% CPU · 4.2 GB · 23 processes');
    const { el: stopped } = await mount('docker-lab');
    expect(stopped.querySelector('.load')).toBeNull();
    runners = runners.map((r) => (r.name === 'ui-tests' && r.usage ? { ...r, usage: { ...r.usage, processes: 1 } } : r));
    expect(text((await mount('ui-tests')).el.querySelector('.load'))).toContain('1 process');
  });

  it('shows a pending action of a busy runner', async () => {
    runners = runners.map((r) => (r.name === 'build-win' ? { ...r, pending: 'restart' as const } : r));
    const { el } = await mount('build-win');
    expect(text(el.querySelector('.note'))).toBe('Restarts after the current job');
  });

  it('shows the problem of a crash-looping runner and flags attention', async () => {
    const { el } = await mount('docker-lab');
    expect(text(el.querySelector('.problem'))).toContain('Exited 5 times in 10 minutes');
    expect(text(el.querySelector('.problem'))).toContain('Next start in 22 min');
    expect(el.querySelector('.tile.attention')).not.toBeNull();
    expect(text(el.querySelector('.disk'))).toMatch(/ low$/);
    expect(el.querySelector('.meta .meter')).toBeNull();
    expect(el.querySelector('.tile > .meter.danger')).not.toBeNull();
    expect(text(el.querySelector('.meta'))).toContain('7 restarts / 24 h');
    expect(el.querySelector('.badge.update')?.getAttribute('title')).toBe('5.3.0 available');
  });

  it('shows ONE failing message: the crash reason with a danger icon and a View logs link, without an inner box', async () => {
    const { el } = await mount('docker-lab');
    const problem = el.querySelector('.problem')!;
    expect(problem.querySelectorAll('p')).toHaveLength(1);
    expect(problem.querySelector('mat-icon.tone-danger')).not.toBeNull();
    expect(problem.querySelector('a.view-logs')?.getAttribute('href')).toBe('/logs?runner=docker-lab');
    expect(text(problem.querySelector('a.view-logs'))).toBe('View logs');
    expect(el.querySelector('.callout')).toBeNull();
  });

  it('puts version, uptime and disk on one unlabelled meta line, tints disk only when low, shows no meter otherwise', async () => {
    const { el } = await mount('build-win');
    const meta = el.querySelector('p.meta')!;
    expect(el.querySelector('dl')).toBeNull();
    expect(meta.textContent).not.toMatch(/Version|Up for|Disk/);
    expect(text(meta)).toBe('5.2.0·up 3d 4h·38.4 GB');
    expect(el.querySelector('.disk.tone-danger')).toBeNull();
    expect(el.querySelector('.meter')).toBeNull();
    expect(text(meta)).not.toContain('available');
  });

  it('shows an update as one arrow token next to the version, not a second badge', async () => {
    const { el } = await mount('ui-tests');
    expect(el.querySelectorAll('.meta .badge')).toHaveLength(1);
    expect(text(el.querySelector('.badge.update'))).toBe('5.3.0');
    expect(text(el)).not.toContain('available');
  });

  it('shows that a paused runner takes no jobs and offers Resume', async () => {
    const { el } = await mount('legacy-ci');
    expect(text(el.querySelector('.quiet'))).toContain('Takes no jobs until you resume it.');
    expect(text(el.querySelector('.quiet'))).toContain('service account');
    const resume = byText<HTMLButtonElement>(el, 'button', 'Resume')!;
    const act = vi.spyOn(mock, 'resume');
    resume.click();
    await flush();
    expect(act).toHaveBeenCalledWith('legacy-ci');
    expect(byText(el, 'button', 'Update now')).toBeNull();
  });

  it('waits for jobs when idle, mentions the last job, and updates now', async () => {
    const lastJob: FinishedJob = { info: { repository: 'a/b', job: 'x', jobId: 1, runId: 1, runNumber: 1, taskId: 1, server: null }, startedAt: NOW - 600_000, finishedAt: NOW - 300_000, peakCpuPermille: 0, peakMemBytes: 0, leftovers: 0 };
    const { el } = await mount('ui-tests', { lastJob });
    expect(text(el.querySelector('.quiet'))).toContain('Waiting for jobs');
    expect(text(el.querySelector('.quiet'))).toContain('Last job finished 5 min ago');
    const apply = vi.spyOn(mock, 'applyUpdate');
    byText<HTMLButtonElement>(el, 'button', 'Update now')!.click();
    await flush();
    expect(apply).toHaveBeenCalledWith('ui-tests');
  });

  it('shows the update phase while updating', async () => {
    runners = runners.map((r) => (r.name === 'ui-tests' ? { ...r, updatePhase: { phase: 'downloading' as const, plan: { from: '5.2.0', to: '5.3.0' } } } : r));
    const { el } = await mount('ui-tests');
    expect(text(el.querySelector('.phase'))).toBe('Downloading · 5.3.0');
    expect(byText(el, 'button', 'Update now')).toBeNull();
  });

  it('shows a stopped runner', async () => {
    runners = runners.map((r) => (r.name === 'ui-tests' ? { ...r, status: 'stopped' as const, disk: null, runningSince: null } : r));
    const { el } = await mount('ui-tests');
    expect(text(el.querySelector('.quiet'))).toBe('Not running.');
    expect(el.querySelector('.meta .disk')).toBeNull();
    expect(el.querySelector('.meta .uptime')).toBeNull();
  });

  it('offers one contextual action: Update now, else Restart, else Resume', async () => {
    const store = TestBed.inject(StatusStore);
    const act = vi.spyOn(store, 'act');
    const { el } = await mount('ui-tests');
    byText<HTMLButtonElement>(el, 'button', 'Update now')!.click();
    expect(act).toHaveBeenCalledWith('ui-tests', 'applyUpdate');
    expect(byText(el, 'button', 'Restart')).toBeNull();
    runners = runners.map((r) => (r.name === 'ui-tests' ? { ...r, updateTarget: null } : r));
    byText<HTMLButtonElement>((await mount('ui-tests')).el, 'button', 'Restart')!.click();
    expect(act).toHaveBeenCalledWith('ui-tests', 'restart');
    runners = runners.map((r) => (r.name === 'ui-tests' ? { ...r, status: 'paused' as const } : r));
    byText<HTMLButtonElement>((await mount('ui-tests')).el, 'button', 'Resume')!.click();
    expect(act).toHaveBeenCalledWith('ui-tests', 'resume');
    await flush();
  });

  it('says why Pitcrew holds a runner, how late an update is and when updates loop', async () => {
    runners = runners.map((r) =>
      r.name === 'ui-tests'
        ? {
            ...r,
            hold: { reasons: ['schedule' as const], paused: true, minutesLeft: 90 },
            deadline: { behindSince: NOW - 40 * 86_400_000, dueAt: NOW - 10 * 86_400_000, daysLeft: -10, state: 'overdue' as const },
            updateLoop: { target: '5.3.0', kind: 'failing' as const, attempts: 3, cause: 'The update failed 3 times in 7 days.' },
          }
        : r,
    );
    const { el } = await mount('ui-tests');
    expect(text(el.querySelector('.hold'))).toBe('Paused by Pitcrew: a pause window is open · 1h 30m left');
    expect(text(el.querySelector('.deadline'))).toBe('Update overdue by 10 days');
    expect(el.querySelector('.deadline.tone-warning')).not.toBeNull();
    expect(text(el.querySelector('.loop'))).toBe('Updating to 5.3.0 keeps failing ·View');
    expect(el.querySelector('.loop a')?.getAttribute('href')).toBe('/updates');
    expect(el.querySelector('.loop .callout')).toBeNull();
    expect(el.querySelector('.hold.badge')).toBeNull();
    runners = runners.map((r) =>
      r.name === 'ui-tests' ? { ...r, hold: { reasons: ['lowDisk' as const], paused: false, minutesLeft: null }, updateLoop: null, deadline: { behindSince: NOW, dueAt: NOW, daysLeft: 4, state: 'soon' as const } } : r,
    );
    const again = (await mount('ui-tests')).el;
    expect(text(again.querySelector('.hold'))).toBe('Pausing after this job: the disk is nearly full');
    expect(text(again.querySelector('.deadline'))).toBe('Update due in 4 days');
    expect(again.querySelector('.deadline.tone-warning')).not.toBeNull();
    expect(again.querySelector('.loop')).toBeNull();
  });

  it('links to the runner; the footer holds only the one contextual action, no Logs link', async () => {
    const { el } = await mount('ui-tests');
    expect(el.querySelector('a.name')?.getAttribute('href')).toBe('/runners/ui-tests');
    const footer = el.querySelector('footer.actions')!;
    expect(footer.querySelectorAll('button, a')).toHaveLength(1);
    expect(text(footer)).not.toContain('Logs');
  });
});

describe('MachineTiles', () => {
  const sample = (i: number): MachineSample => ({
    at: i,
    cpuPermille: 100 * i,
    cpuPeakPermille: 150 * i,
    memUsedBytes: 8 * 1000 ** 3,
    memTotalBytes: 16 * 1000 ** 3,
    diskFreeBytes: 50 * 1000 ** 3,
    diskTotalBytes: 500 * 1000 ** 3,
    runners: { cpuPermille: 50 * i, cpuPeakPermille: 80 * i, memBytes: 2 * 1000 ** 3, memPeakBytes: 3 * 1000 ** 3, processes: 4 },
  });
  const service = (machine: MachineSample | null): ServiceInfo => ({
    version: '0.1.0',
    startedAt: NOW - 3_600_000,
    dataDir: 'C:\\x',
    configProblems: [],
    readOnly: false,
    machine,
    ownCpuPermille: 25,
    ownMemBytes: 24 * 1000 ** 2,
  });

  beforeEach(() => TestBed.configureTestingModule({ providers: appProviders(makeMock()) }));

  function mount(samples: MachineSample[], svc: ServiceInfo | null) {
    const f = TestBed.createComponent(MachineTiles);
    f.componentRef.setInput('samples', samples);
    f.componentRef.setInput('service', svc);
    f.componentRef.setInput('now', NOW);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }

  it('shows the runners CPU and memory, the machine free disk, and the service cost', () => {
    const el = mount([sample(1), sample(2), sample(3)], service(null));
    const tiles = Array.from(el.querySelectorAll('section')).map((s) => s.getAttribute('aria-label'));
    expect(tiles[0]).toBe('Runners CPU: 15%, avg 10% · peak 24%');
    expect(tiles[1]).toBe('Runners memory: 2.0 GB, 13% of 16 GB · peak 3.0 GB');
    expect(tiles[2]).toBe('Free disk: 50.0 GB, of 500 GB · 10% free');
    expect(text(el.querySelector('.own'))).toContain('2.5% CPU');
    expect(text(el.querySelector('.own'))).toContain('24.0 MB memory · up 1h 00m');
    expect(el.querySelectorAll('pc-sparkline')).toHaveLength(3);
  });

  it('adds the whole machine as a muted context line, runners included', () => {
    const el = mount([sample(1), sample(2), sample(3)], service(null));
    expect(text(el.querySelector('[data-testid="machine-context"]'))).toBe('This machine: 30% CPU · 8.0 GB of 16 GB memory in use, runners included');
    expect(text(mount([], null).querySelector('[data-testid="machine-context"]'))).toContain('appear once samples arrive');
  });

  it('falls back to the service sample, and to dashes without data', () => {
    const el = mount([], service(sample(5)));
    expect(el.querySelector('section')?.getAttribute('aria-label')).toBe('Runners CPU: 25%, No samples yet');
    const empty = mount([], null);
    expect(empty.querySelector('section')?.getAttribute('aria-label')).toBe('Runners CPU: —, No samples yet');
    expect(text(empty.querySelector('.own'))).toContain('— CPU');
  });

  it('warns about low free disk', () => {
    const low = { ...sample(1), diskFreeBytes: 10 * 1000 ** 3 };
    const el = mount([low], service(null));
    const spark = el.querySelectorAll('pc-sparkline')[2] as HTMLElement;
    expect(spark.style.color).toContain('--warning');
  });
});

describe('EventsFeed', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: appProviders(makeMock()) }));

  function mount(items: FeedItem[]) {
    const f = TestBed.createComponent(EventsFeed);
    f.componentRef.setInput('items', items);
    f.componentRef.setInput('now', NOW);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }

  it('says when nothing happened', () => {
    expect(text(mount([]))).toContain('Nothing happened in this range');
  });

  it('lists events with runner and relative time', () => {
    const el = mount([
      { key: 'a', at: NOW - 3_600_000, runner: 'build-win', tone: 'danger', icon: 'x-octagon', title: 'Crashed and restarted', detail: 'Exited with code 1.' },
      { key: 'b', at: NOW - 60_000, runner: 'ui-tests', tone: 'success', icon: 'play', title: 'Resumed', detail: 'Takes jobs again.' },
    ]);
    const rows = el.querySelectorAll('li');
    expect(rows).toHaveLength(2);
    expect(text(rows[0])).toContain('Crashed and restarted');
    expect(text(rows[0])).toContain('build-win · 1 h ago');
    expect(rows[0]!.querySelector('time')?.getAttribute('datetime')).toBe(new Date(NOW - 3_600_000).toISOString());
    expect(rows[1]!.querySelector('.icon')?.classList.contains('success')).toBe(true);
  });
});

it('provides the mock for the dashboard components', () => {
  TestBed.configureTestingModule({ providers: appProviders(makeMock()) });
  expect(TestBed.inject(BACKEND)).toBeInstanceOf(MockBackend);
});
