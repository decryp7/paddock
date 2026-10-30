import type { Job, Overview } from '../../core/api.types';
import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { provideRouter } from '@angular/router';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { settle } from '../../../testing/render';

import { QueueCard } from './queue-card';

const NOW = 1_800_000_000_000;
const job = (over: Partial<Job> = {}): Job => ({
  id: 'j1',
  appId: 'debrief',
  modelId: 'm',
  priority: 'interactive',
  kind: 'chat',
  state: 'running',
  submittedAt: NOW - 5_000,
  startedAt: NOW - 3_000,
  progress: null,
  tokens: 0,
  ...over,
});

async function mount(patch?: (o: Overview) => Overview, inputs: { max?: number } = {}, backend = new MockBackend({ now: () => NOW })) {
  TestBed.configureTestingModule({ imports: [QueueCard], providers: [{ provide: BACKEND, useValue: backend }, provideRouter([])] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
  if (patch) patchState(live as unknown as WritableStateSource<object>, { overview: patch(live.overview()!) });
  const fixture = TestBed.createComponent(QueueCard);
  fixture.componentRef.setInput('now', NOW);
  if (inputs.max !== undefined) fixture.componentRef.setInput('max', inputs.max);
  await settle(fixture);
  return { fixture, el: fixture.nativeElement as HTMLElement, live, backend };
}

const withQueue = (running: Job[], waiting: Job[]) => (o: Overview): Overview => ({
  ...o,
  queue: { running, waiting, depth: { interactive: waiting.filter((j) => j.priority === 'interactive').length, background: 1, batch: 0 } },
});

describe('QueueCard', () => {
  it('shows nothing before the overview is loaded', async () => {
    TestBed.configureTestingModule({ imports: [QueueCard], providers: [{ provide: BACKEND, useValue: new MockBackend({ failing: ['overview'] }) }] });
    const live = TestBed.inject(LiveStore);
    await live.load();
    const fixture = TestBed.createComponent(QueueCard);
    fixture.componentRef.setInput('now', NOW);
    await settle(fixture);
    expect((fixture.nativeElement as HTMLElement).querySelector('.depth')).toBeNull();
    expect(live.error()?.code).toBe('serverUnavailable');
  });

  it('shows the depth by priority and the running then waiting jobs with app names', async () => {
    const { el } = await mount();
    // The priority split is one muted line, not three tiles.
    expect(el.querySelector('.depth')!.textContent!.replace(/\s+/g, ' ').trim()).toMatch(/^\d+ interactive\d+ background\d+ batch$/);
    expect(el.querySelectorAll('.depth .d').length).toBe(0);
    const rows = el.querySelectorAll('.jobs li');
    expect(rows.length).toBe(4);
    expect(rows[0]!.querySelector('.job-dot')!.getAttribute('aria-label')).toBe('Running');
    expect(rows[0]!.querySelector('.app')!.textContent).toBe('Debrief');
    expect(rows[0]!.querySelector('.kind')!.textContent).toBe('chat');
    expect(el.querySelector('.more')!.textContent).toContain('and 1 more waiting');
  });

  it('shows running time, token counts and progress for a running job', async () => {
    const { el } = await mount(withQueue([job({ progress: 0.42, tokens: 182_400, startedAt: NOW - 80_000 })], []));
    const line = el.querySelector('.line2')!.textContent!.replace(/\s+/g, ' ');
    expect(line).toContain('Interactive');
    expect(line).toContain('1 min');
    expect(line).toMatch(/182(\.4)?k tok/);
    const bar = el.querySelector('.prog')!;
    expect(bar.getAttribute('aria-valuenow')).toBe('42');
    expect(bar.getAttribute('aria-label')).toBe('Progress of Debrief chat');
    expect(bar.tagName).toBe('MAT-PROGRESS-BAR');
    expect(bar.getAttribute('mode')).toBe('determinate');
  });

  it('measures a running job from submission when it has no start time, and hides zero tokens', async () => {
    const { el } = await mount(withQueue([job({ startedAt: null, submittedAt: NOW - 2_000, tokens: 0 })], []));
    const line = el.querySelector('.line2')!.textContent!;
    expect(line).toContain('2.0 s');
    expect(line).not.toContain('tok');
    expect(el.querySelector('.prog')).toBeNull();
  });

  it('shows how long a waiting job has waited and no progress bar', async () => {
    const { el } = await mount(withQueue([], [job({ id: 'w', state: 'queued', startedAt: null, submittedAt: NOW - 12_000, progress: 0 })]));
    expect(el.querySelector('.line2')!.textContent).toContain('waiting 12 s');
    expect(el.querySelector('.job-dot')!.getAttribute('aria-label')).toBe('Waiting');
    expect(el.querySelector('.prog')).toBeNull();
  });

  it('falls back to the app id for an unknown app', async () => {
    const { el } = await mount(withQueue([job({ appId: 'ghost' })], []));
    expect(el.querySelector('.app')!.textContent).toBe('ghost');
  });

  it('says so when nothing is queued', async () => {
    const { el } = await mount(withQueue([], []));
    expect(el.querySelector('.none')!.textContent).toContain('Nothing running or waiting');
    expect(el.querySelector('.more')).toBeNull();
  });

  it('lists at most max jobs and counts the rest', async () => {
    const { el } = await mount(undefined, { max: 2 });
    expect(el.querySelectorAll('.jobs li').length).toBe(2);
    expect(el.querySelector('.more')!.textContent).toContain('and 3 more waiting');
  });

  it('cancels a job through the live store and disables its button meanwhile', async () => {
    const { fixture, el, backend, live } = await mount();
    const spy = vi.spyOn(backend, 'cancelJob');
    const btn = el.querySelector('button[aria-label="Cancel Debrief chat"]') as HTMLButtonElement;
    btn.click();
    fixture.detectChanges();
    expect(btn.disabled).toBe(true);
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('job-41');
    expect(live.isBusy()('job:job-41')).toBe(false);
  });
});
