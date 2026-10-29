import { TestBed } from '@angular/core/testing';
import { ContributionRegistry, provideContributions } from '@decryp7/paddock-ui/shell';
import { assertContributionsValid } from '@decryp7/paddock-ui/shell/testing';
import { Injector } from '@angular/core';
import { JobsBackend, type Unlisten } from './jobs-backend';
import { JOBS_CONTRIBUTIONS } from './jobs.contributions';
import { PdJobsList } from './jobs-list';
import { PdJobsStatus } from './jobs-status';
import { jobsStatusDetail, jobsStatusProgress, jobsStatusText } from './jobs-status-text';
import { jobsBed, phase, settle, snap } from './spec-helpers';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';

class FailingBackend extends JobsBackend {
  fail = true;
  snapshot(): Promise<readonly SuJobSnapshot[]> {
    return this.fail ? Promise.reject(new Error('the engine is gone')) : Promise.resolve([snap(1, 1)]);
  }
  cancel(): Promise<boolean> {
    return Promise.resolve(false);
  }
  listen(): Promise<Unlisten> {
    return Promise.resolve(() => undefined);
  }
}

class NeverBackend extends FailingBackend {
  override snapshot(): Promise<readonly SuJobSnapshot[]> {
    return new Promise(() => undefined);
  }
}

describe('pd-jobs-list', () => {
  it('lists every job with its phase, "Still working" when quiet, and Cancel', async () => {
    const { backend, clock } = jobsBed({ stillWorkingAfterMs: 2_000 });
    const cancel = vi.spyOn(backend, 'cancel');
    backend.emit(snap(1, 1, 'running', { title: 'Scanning the disk', phase: phase('Finding files', 12_345) }));
    backend.emit(snap(2, 1, 'done', { title: 'Exporting' }));
    const fixture = TestBed.createComponent(PdJobsList);
    await settle();
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    const items = el.querySelectorAll('li');
    expect(el.querySelector('ul')?.getAttribute('aria-label')).toBe('Jobs');
    expect(items).toHaveLength(2);
    expect(items[0]?.textContent).toContain('Finding files');
    expect(items[0]?.textContent).toContain('12,345 so far');
    expect(items[1]?.textContent).toContain('Done');
    clock.advance(2_000);
    await fixture.whenStable();
    expect(items[0]?.textContent).toContain('Still working');
    items[0]?.querySelector<HTMLButtonElement>('button.pd-cancel')?.click();
    expect(cancel).toHaveBeenCalledWith(1);
  });

  it('shows an empty state when there are no jobs', async () => {
    jobsBed();
    const fixture = TestBed.createComponent(PdJobsList);
    await settle();
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('No jobs');
  });

  it('shows a skeleton while the first list loads', async () => {
    jobsBed({ backend: new NeverBackend() });
    const fixture = TestBed.createComponent(PdJobsList);
    fixture.componentRef.setInput('label', 'Background work');
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('[aria-busy="true"]')?.getAttribute('aria-label')).toBe('Background work');
  });

  it('shows a failed list inline with Retry', async () => {
    const backend = new FailingBackend();
    jobsBed({ backend });
    const fixture = TestBed.createComponent(PdJobsList);
    await settle();
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.textContent).toContain('The jobs could not be read');
    expect(el.textContent).toContain('the engine is gone');
    expect(el.textContent).not.toContain('No jobs');
    backend.fail = false;
    [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Retry'))?.click();
    await settle();
    await fixture.whenStable();
    expect(el.querySelectorAll('li')).toHaveLength(1);
    expect(el.textContent).not.toContain('could not be read');
  });
});

describe('pd-jobs-status', () => {
  it('says how many jobs run and what the first one does', () => {
    expect(jobsStatusText(0, null)).toBe('Jobs');
    expect(jobsStatusText(1, null)).toBe('1 running');
    expect(jobsStatusText(1200, 'Update runner')).toBe('1,200 running: Update runner');
    expect(jobsStatusDetail(null, 'Reading', false)).toBe('');
    expect(jobsStatusDetail('Import', null, false)).toBe('Import');
    expect(jobsStatusDetail('Import', 'Reading', false)).toBe('Import: Reading');
    expect(jobsStatusDetail('Import', 'Reading', true)).toBe('Import: still working, this step is taking longer than usual');
  });

  it('opens the Jobs list in a popover; Escape closes it and returns focus, a click outside closes it', async () => {
    const { backend, clock } = jobsBed({ stillWorkingAfterMs: 1_000 });
    backend.emit(snap(1, 1, 'queued'));
    const fixture = TestBed.createComponent(PdJobsStatus);
    document.body.appendChild(fixture.nativeElement);
    await settle();
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    const button = el.querySelector<HTMLButtonElement>('button.pd-pill') as HTMLButtonElement;
    expect(button.textContent?.trim()).toBe('1 running');
    backend.emit(snap(2, 1, 'running'));
    await fixture.whenStable();
    expect(button.textContent?.trim()).toBe('2 running: Job 2');
    expect(button.title).toBe('Job 2: Starting');
    backend.emit(snap(2, 2, 'running', { phase: phase('Reading', 1) }));
    await settle();
    await fixture.whenStable();
    expect(button.title).toBe('Job 2: Reading');
    clock.advance(1_000);
    await fixture.whenStable();
    expect(button.textContent?.trim()).toBe('2 running: Job 2 · Reading 1 so far');
    expect(button.title).toBe('Job 2: still working, this step is taking longer than usual');
    expect(button.dataset['stalled']).toBe('true');

    expect(button.getAttribute('aria-expanded')).toBe('false');
    button.click();
    await fixture.whenStable();
    expect(button.getAttribute('aria-expanded')).toBe('true');
    const panel = el.querySelector('[role="region"]') as HTMLElement;
    expect(panel.id).toBe(button.getAttribute('aria-controls'));
    expect(panel.querySelectorAll('li')).toHaveLength(2);

    panel.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await fixture.whenStable();
    expect(el.querySelector('[role="region"]')).not.toBeNull();
    panel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await fixture.whenStable();
    expect(el.querySelector('[role="region"]')).toBeNull();
    expect(document.activeElement).toBe(button);

    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    button.click();
    await fixture.whenStable();
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await fixture.whenStable();
    expect(el.querySelector('[role="region"]')).toBeNull();
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    fixture.nativeElement.remove();
  });

  it('draws the lead job\'s phase with a determinate ring on a known scale, and a spinner while the total is unknown', async () => {
    const { backend } = jobsBed();
    backend.emit(snap(1, 1, 'running', { title: 'Import', phase: phase('Reading', 40, 100) }));
    const fixture = TestBed.createComponent(PdJobsStatus);
    document.body.appendChild(fixture.nativeElement);
    await settle();
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    const ring = el.querySelector<HTMLElement>('.pd-dot')!;
    expect(el.querySelector('button.pd-pill')!.textContent?.trim()).toBe('1 running: Import · Reading 40%');
    expect(ring.classList.contains('pd-ring')).toBe(true);
    expect(ring.style.getPropertyValue('--pd-jobs-progress')).toBe('40');
    backend.emit(snap(1, 2, 'running', { title: 'Import', phase: phase('Finding files', 1_240) }));
    await settle();
    await fixture.whenStable();
    expect(ring.classList.contains('pd-ring')).toBe(false);
    expect(el.querySelector('button.pd-pill')!.textContent?.trim()).toBe('1 running: Import · Finding files 1,240 so far');
    // A step with nothing to count states its time: the phase and the time are separated (screenshot review P3), a count is not.
    backend.emit(snap(1, 3, 'running', { title: 'Import', phase: phase('Waiting for the device', 0) }));
    await settle();
    await fixture.whenStable();
    expect(el.querySelector('button.pd-pill')!.textContent?.trim()).toMatch(/^1 running: Import · Waiting for the device · \d+ s elapsed$/);
    fixture.nativeElement.remove();
  });

  it('the running spinner and the ring are 14 px with a 2 px stroke (review 2, P3-9)', () => {
    // jsdom does no layout, so the rule is read from the component's styles; e2e/shell-demo.spec.ts measures it in a browser.
    // Angular keeps the styles scoped and pretty-printed (`.a[_ngcontent-%COMP%] {\n  width: 14px;`): flatten them to `.a { width: 14px; }`.
    const css = ((PdJobsStatus as unknown as { ɵcmp: { styles: string[] } }).ɵcmp.styles ?? []).join('\n').replace(/\[_ng(?:content|host)-%COMP%\]/g, '').replace(/\s+/g, ' ');
    const busy = /\.pd-busy \.pd-dot \{([^}]*)\}/.exec(css)?.[1] ?? '';
    expect(busy).toMatch(/box-sizing: border-box; width: 14px; height: 14px;/);
    expect(busy).toMatch(/border: 2px solid/);
    // The ring's hole is 14 px wide minus a 2 px band: a 5 px radius.
    expect(css).toMatch(/radial-gradient\(\s*circle, transparent 4\.5px, var\(\s*--(?:%NS%)?pd-text\s*\) 5px\)/);
  });

  it('opens below when the item sits at the top', async () => {
    jobsBed();
    const fixture = TestBed.createComponent(PdJobsStatus);
    fixture.componentRef.setInput('placement', 'below');
    await fixture.whenStable();
    expect(fixture.nativeElement.classList).toContain('pd-below');
    expect(fixture.nativeElement.querySelector('button')?.textContent?.trim()).toBe('Jobs');
  });
});

describe('JOBS_CONTRIBUTIONS', () => {
  it('passes the contributions lint and loads the status item lazily', async () => {
    jobsBed({ extra: [provideContributions(JOBS_CONTRIBUTIONS)] });
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
    const registry = TestBed.inject(ContributionRegistry);
    const item = registry.find('statusItems', 'pd.jobs') as { load: () => Promise<unknown> } | undefined;
    expect(await item?.load()).toBe(PdJobsStatus);
    expect(registry.has('notificationKinds', 'pd.jobs.failed')).toBe(true);
  });
});

describe('the status item does not repeat the job title', () => {
  const view = { headline: 'Downloading', percent: 40, count: '', timed: false };
  it('leaves the phase out when it is the title', () => {
    expect(jobsStatusProgress(view, 'Downloading')).toBe('40%');
    expect(jobsStatusProgress(view, 'Update runner')).toBe('Downloading 40%');
    expect(jobsStatusProgress({ ...view, percent: null, count: '1,240 so far' }, 'downloading')).toBe('1,240 so far');
  });
});
