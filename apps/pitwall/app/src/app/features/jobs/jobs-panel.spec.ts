import { TestBed } from '@angular/core/testing';
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
    el.querySelector<HTMLButtonElement>('button[aria-label^="Stop waiting"]')!.click();
    expect(await pending).toMatchObject({ code: 'cancelled', message: expect.stringContaining('may still be working') });
    fixture.detectChanges();
    expect(el.querySelector('.jobs')).toBeNull();
  });
});
