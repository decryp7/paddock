import { TestBed } from '@angular/core/testing';
import type { JobEvent } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { JobsPanel } from './jobs-panel';

function setup() {
  const backend = new MockBackend();
  backend.holdJobs = true;
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
  const fixture = TestBed.createComponent(JobsPanel);
  fixture.detectChanges();
  return { backend, fixture, el: fixture.nativeElement as HTMLElement };
}

const text = (el: HTMLElement) => (el.textContent ?? '').replace(/\s+/g, ' ');

describe('JobsPanel', () => {
  afterEach(() => vi.useRealTimers());

  it('renders nothing while no job runs', () => {
    expect(setup().el.querySelector('.jobs')).toBeNull();
  });

  it('shows a running job with its phase, a growing count and Cancel, and drops it when it ends', async () => {
    const { backend, fixture, el } = setup();
    const pending = backend.loadModel('gemma-4-12b-it-q4_k_m').catch(() => undefined);
    fixture.detectChanges();
    expect(text(el)).toContain('Loading the model');
    expect(text(el)).toContain('1 s');
    expect(el.querySelector('mat-progress-bar')).not.toBeNull();
    const cancel = el.querySelector<HTMLButtonElement>('button[aria-label^="Cancel"]')!;
    cancel.click();
    await pending;
    fixture.detectChanges();
    expect(el.querySelector('.jobs')).toBeNull();
  });

  it('says Still working after a quiet spell, and never times the job out', async () => {
    vi.useFakeTimers();
    const { backend, fixture, el } = setup();
    void backend.runBenchmark('qwen3.5-4b-q4_k_m').catch(() => undefined);
    fixture.detectChanges();
    expect(text(el)).not.toContain('Still working');
    await vi.advanceTimersByTimeAsync(11_000);
    fixture.detectChanges();
    expect(text(el)).toContain('Still working');
    await vi.advanceTimersByTimeAsync(3_600_000);
    fixture.detectChanges();
    expect(el.querySelector('.job')).not.toBeNull();
  });

  it('offers Stop waiting once Cancel did not take effect, and the job ends when the user stops waiting', async () => {
    const { backend, fixture, el } = setup();
    backend.refuseCancel = true;
    const pending = backend.loadModel('gemma-4-12b-it-q4_k_m').catch((e: unknown) => e);
    fixture.detectChanges();
    expect(text(el)).not.toContain('Stop waiting');
    el.querySelector<HTMLButtonElement>('button[aria-label^="Cancel"]')!.click();
    await Promise.resolve();
    await Promise.resolve();
    fixture.detectChanges();
    expect(text(el)).toContain('Stop waiting');
    expect(text(el)).toContain('Cancel did not take effect');
    expect(el.querySelector<HTMLButtonElement>('button[aria-label^="Cancel"]')!.disabled).toBe(false);
    el.querySelector<HTMLButtonElement>('button[aria-label^="Stop waiting"]')!.click();
    expect(await pending).toMatchObject({ code: 'cancelled', message: expect.stringContaining('may still be working') });
    fixture.detectChanges();
    expect(el.querySelector('.jobs')).toBeNull();
  });

  it('disables Cancel with the reason while the backend says the job cannot be cancelled', async () => {
    const { backend, fixture, el } = setup();
    void backend.installService().catch(() => undefined);
    backend['emitJob']({ kind: 'progress', jobId: 1, phase: 'Waiting for the system prompt', done: 1, total: null, unit: 'items', canStopWaiting: false, elapsedSecs: 1, cancellable: false, notCancellableReason: 'Answer the system prompt.' });
    fixture.detectChanges();
    const cancel = el.querySelector<HTMLButtonElement>('button[aria-label^="Cancel"]')!;
    expect(cancel.disabled).toBe(true);
    const reason = document.getElementById(cancel.getAttribute('aria-describedby')!)!;
    expect(reason.textContent).toContain('Answer the system prompt.');
  });

  it('keeps one polite live region present, names Cancel by the job and the phase, and labels the bar like the visible text', () => {
    const { backend, fixture, el } = setup();
    const live = el.querySelector('[role="status"][aria-live="polite"]');
    expect(live).not.toBeNull();
    void backend.loadModel('gemma-4-12b-it-q4_k_m').catch(() => undefined);
    fixture.detectChanges();
    expect(el.querySelector('[role="status"][aria-live="polite"]')).toBe(live);
    expect(live!.textContent).toContain('Loading the model');
    expect(el.querySelector('button[aria-label^="Cancel"]')!.getAttribute('aria-label')).toBe('Cancel: Loading the model');
    expect(el.querySelector('mat-progress-bar')!.getAttribute('aria-label')).toBe('Loading the model: Working on it');
  });

  it('shows the backend phase, not a local label, while Cancel is pending', async () => {
    const listeners: ((e: JobEvent) => void)[] = [];
    const backend = { jobEvents: (l: (e: JobEvent) => void) => (listeners.push(l), () => undefined), jobCancel: () => new Promise<boolean>(() => undefined), jobStopWaiting: () => Promise.resolve(false) };
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
    const fixture = TestBed.createComponent(JobsPanel);
    const el = fixture.nativeElement as HTMLElement;
    listeners[0]!({ kind: 'started', jobId: 1, title: 'Updating Pitwall' });
    listeners[0]!({ kind: 'progress', jobId: 1, phase: 'Writing the weights', done: 1, total: null, unit: 'items', canStopWaiting: false, elapsedSecs: 1, cancellable: true, notCancellableReason: null });
    fixture.detectChanges();
    el.querySelector<HTMLButtonElement>('button[aria-label^="Cancel"]')!.click();
    fixture.detectChanges();
    expect(el.querySelector('.phase')!.textContent).toBe('Writing the weights');
    expect(el.querySelector<HTMLButtonElement>('button[aria-label^="Cancel"]')!.disabled).toBe(true);
  });
});
