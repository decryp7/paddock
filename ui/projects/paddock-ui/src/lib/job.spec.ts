import { TestBed } from '@angular/core/testing';
import { SuJob } from './job';
import { formatBytes, formatCount, formatElapsed, formatEta, jobView, type SuJobSnapshot } from './job-view';

const base = { id: 7, kind: 'import', title: 'Import logs', subject: '3 folders', seq: 1, elapsedMs: 0, cancellable: true };
const running = (phase: unknown, extra: Partial<SuJobSnapshot> = {}): SuJobSnapshot =>
  ({ ...base, state: 'running', phase, ...extra }) as SuJobSnapshot;
const discovering = running({ id: 'walk', label: 'Finding files', unit: 'items', total: null, done: 12345 });
const download = running({ id: 'dl', label: 'Downloading', unit: 'bytes', total: 118 * 1024 * 1024, done: 42 * 1024 * 1024 }, { etaMs: 90_000 });

function render(job: SuJobSnapshot, stalled = false) {
  const f = TestBed.createComponent(SuJob);
  f.componentRef.setInput('job', job);
  f.componentRef.setInput('stalled', stalled);
  f.detectChanges();
  return f;
}
const text = (el: Element) => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

describe('formatters', () => {
  it('formats counts', () => {
    expect(formatCount(12345)).toBe('12,345');
    expect(formatCount(-3)).toBe('0');
    expect(formatCount(NaN)).toBe('0');
  });
  it('formats sizes', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(4.2 * 1024 * 1024)).toBe('4.2 MB');
    expect(formatBytes(118 * 1024 * 1024)).toBe('118 MB');
    expect(formatBytes(5 * 1024 ** 5)).toBe('5120 TB');
    expect(formatBytes(NaN)).toBe('0 B');
  });
  it('formats time left', () => {
    expect(formatEta(null)).toBe('');
    expect(formatEta(undefined)).toBe('');
    expect(formatEta(-1)).toBe('');
    expect(formatEta(Infinity)).toBe('');
    expect(formatEta(12_000)).toBe('12 s left');
    expect(formatEta(180_000)).toBe('about 3 min left');
    expect(formatEta(3_600_000)).toBe('about 1 h left');
    expect(formatEta(3_900_000)).toBe('about 1 h 5 min left');
  });
});

describe('jobView', () => {
  it('queued waits and can be cancelled', () => {
    const v = jobView({ ...base, state: 'queued' } as SuJobSnapshot);
    expect([v.headline, v.canCancel, v.showBar]).toEqual(['Waiting', true, false]);
  });
  it('running without a phase says Starting with an indeterminate bar', () => {
    const v = jobView({ ...base, state: 'running' } as SuJobSnapshot);
    expect([v.headline, v.percent, v.showBar, v.count]).toEqual(['Starting', null, true, '0 s elapsed']);
  });
  it('unknown total is indeterminate with a growing count and no ETA', () => {
    const v = jobView({ ...discovering, etaMs: 5000 } as SuJobSnapshot);
    expect([v.percent, v.count, v.eta]).toEqual([null, '12,345 so far', '']);
  });
  it('an empty indeterminate phase counts its time instead (rule 11: never a static value)', () => {
    expect(jobView(running({ id: 'a', label: 'A', unit: 'items', done: 0 })).count).toBe('0 s elapsed');
    expect(jobView(running({ id: 'a', label: 'A', unit: 'items', done: 0 }), true, 72_000).count).toBe('1 min elapsed');
    expect(jobView(running({ id: 'a', label: 'A', unit: 'items', total: 4, done: 0 }), false, 72_000).count).toBe('0 of 4');
    expect(jobView(running({ id: 'a', label: 'A', unit: 'items', total: 0, done: 0 })).percent).toBeNull();
  });
  it('known total is determinate with sizes and ETA', () => {
    const v = jobView(download);
    expect(v.percent).toBeCloseTo(35.59, 1);
    expect([v.count, v.eta]).toEqual(['42 MB of 118 MB', 'about 2 min left']);
  });
  it('clamps done past the total', () => {
    const v = jobView(running({ id: 'a', label: 'A', unit: 'items', total: 10, done: 99 }));
    expect([v.percent, v.count]).toEqual([100, '10 of 10']);
  });
  it('an uninterruptible step disables Cancel and says why', () => {
    const v = jobView(running({ id: 'a', label: 'Saving', unit: 'items', total: 1, done: 0 }, { cancellable: false, notCancellableReason: 'Writing the database' }));
    expect([v.canCancel, v.cancelDisabled, v.cancelHint]).toEqual([true, true, 'Writing the database']);
    expect(jobView(running({ id: 'a', label: 'S', unit: 'items', done: 0 }, { cancellable: false })).cancelHint).toBe('This step cannot be stopped');
  });
  it('stalled is a warning, not an error', () => {
    const v = jobView(discovering, true);
    expect([v.tone, v.detail]).toEqual(['warning', 'Still working. This step is taking longer than usual.']);
  });
  it('terminal states', () => {
    const done = jobView({ ...base, state: 'done' } as SuJobSnapshot);
    const failed = jobView({ ...base, state: 'failed', message: 'Disk full' } as SuJobSnapshot);
    const user = jobView({ ...base, state: 'cancelled', reason: 'user' } as SuJobSnapshot);
    const late = jobView({ ...base, state: 'cancelled', reason: 'deadline' } as SuJobSnapshot);
    expect([done.tone, done.terminal, done.canCancel]).toEqual(['success', true, false]);
    expect([failed.tone, failed.detail]).toEqual(['danger', 'Disk full']);
    expect(user.detail).toBe('Stopped at your request.');
    expect(late.detail).toBe('Stopped: the time limit was reached.');
  });
  it('the announcement does not change when only the count changes', () => {
    const a = jobView(running({ id: 'w', label: 'Finding files', unit: 'items', done: 1 })).announcement;
    const b = jobView(running({ id: 'w', label: 'Finding files', unit: 'items', done: 9999 })).announcement;
    expect(a).toBe(b);
    expect(jobView({ ...base, state: 'queued' } as SuJobSnapshot).announcement).toContain('waiting');
  });
});

describe('SuJob', () => {
  it('renders an indeterminate bar with the growing count and an accessible name', () => {
    const el = render(discovering).nativeElement as HTMLElement;
    const bar = el.querySelector('[role=progressbar]')!;
    expect(bar.classList.contains('pd-indeterminate')).toBe(true);
    expect(bar.hasAttribute('aria-valuenow')).toBe(false);
    expect(bar.getAttribute('aria-valuetext')).toBe('12,345 so far');
    expect(bar.getAttribute('aria-label')).toBe('Import logs: Finding files');
    expect(text(el)).toContain('Finding files');
    expect(text(el)).toContain('12,345 so far');
    expect(text(el)).toContain('3 folders');
  });
  it('indeterminate without any count says how long it has run', () => {
    const el = render(running({ id: 'a', label: 'A', unit: 'items', done: 0 })).nativeElement as HTMLElement;
    expect(el.querySelector('[role=progressbar]')!.getAttribute('aria-valuetext')).toBe('0 s elapsed');
  });
  it('a stalled step says so with an icon, and counts the time the caller tracks', () => {
    const f = render(running({ id: 'a', label: 'Waiting', unit: 'items', done: 0 }), true);
    f.componentRef.setInput('elapsedMs', 14_000);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.pd-detail svg.pd-warn')).not.toBeNull();
    expect(text(el)).toContain('14 s elapsed');
  });
  it('formats elapsed time', () => {
    expect([formatElapsed(8_400), formatElapsed(3 * 60_000), formatElapsed(65 * 60_000), formatElapsed(120 * 60_000), formatElapsed(Number.NaN)]).toEqual([
      '8 s',
      '3 min',
      '1 h 5 min',
      '2 h',
      '0 s',
    ]);
  });
  it('renders a determinate bar with aria-valuenow, count and ETA', () => {
    const el = render(download).nativeElement as HTMLElement;
    const bar = el.querySelector('[role=progressbar]')!;
    expect(bar.getAttribute('aria-valuenow')).toBe('36');
    expect(bar.hasAttribute('aria-valuetext')).toBe(false);
    expect(text(el)).toContain('42 MB of 118 MB');
    expect(text(el)).toContain('about 2 min left');
  });
  it('Cancel emits the job id', () => {
    const f = render(download);
    let got = -1;
    f.componentInstance.cancel.subscribe((id) => (got = id));
    (f.nativeElement as HTMLElement).querySelector<HTMLButtonElement>('button.pd-cancel')!.click();
    expect(got).toBe(7);
  });
  it('a disabled Cancel shows the reason as text', () => {
    const el = render(running({ id: 'a', label: 'Saving', unit: 'items', done: 0 }, { cancellable: false, notCancellableReason: 'Writing the database' })).nativeElement as HTMLElement;
    expect(el.querySelector<HTMLButtonElement>('button.pd-cancel')!.disabled).toBe(true);
    expect(text(el)).toContain('Writing the database');
  });
  it('terminal jobs have no bar and no Cancel; failure shows its message', () => {
    const el = render({ ...base, state: 'failed', message: 'Disk full' } as SuJobSnapshot).nativeElement as HTMLElement;
    expect(el.querySelector('[role=progressbar]')).toBeNull();
    expect(el.querySelector('button')).toBeNull();
    expect(text(el)).toContain('Failed');
    expect(text(el)).toContain('Disk full');
    expect(el.className).toContain('pd-job--danger');
    expect(el.getAttribute('data-state')).toBe('failed');
  });
  it('has a polite live region and shows Still working when stalled', () => {
    const el = render(discovering, true).nativeElement as HTMLElement;
    expect(el.querySelector('[aria-live=polite]')!.textContent).toContain('Import logs: Finding files');
    expect(text(el)).toContain('Still working');
  });
  it('queued job renders without a bar but with Cancel', () => {
    const el = render({ ...base, state: 'queued' } as SuJobSnapshot).nativeElement as HTMLElement;
    expect(el.querySelector('[role=progressbar]')).toBeNull();
    expect(el.querySelector('button.pd-cancel')).not.toBeNull();
  });
});
