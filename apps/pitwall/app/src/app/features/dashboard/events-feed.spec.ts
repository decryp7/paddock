import type { PitwallEvent } from '../../core/api.types';
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

import { EventsFeed } from './events-feed';

const NOW = 1_800_000_000_000;
const ev = (id: number, over: Partial<PitwallEvent> = {}): PitwallEvent => ({ id, at: NOW - 120_000, kind: 'modelLoaded', severity: 'info', message: 'Loaded', modelId: null, appId: null, reason: null, ...over });

async function mount(events: PitwallEvent[] | null, inputs: { limit?: number } = {}) {
  TestBed.configureTestingModule({ imports: [EventsFeed], providers: [{ provide: BACKEND, useValue: new MockBackend({ now: () => NOW, empty: events === null }) }] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
  if (events) patchState(live as unknown as WritableStateSource<object>, { events });
  const fixture = TestBed.createComponent(EventsFeed);
  fixture.componentRef.setInput('now', NOW);
  if (inputs.limit !== undefined) fixture.componentRef.setInput('limit', inputs.limit);
  await settle(fixture);
  return { fixture, el: fixture.nativeElement as HTMLElement, live };
}

describe('EventsFeed', () => {
  it('shows the empty state when nothing happened yet', async () => {
    const { el } = await mount(null);
    expect(el.querySelector('pw-empty-state')!.textContent).toContain('No events yet');
    expect(el.querySelectorAll('li').length).toBe(0);
  });

  it('lists events with message, relative time, icon and tone per severity', async () => {
    const { el } = await mount([ev(1, { message: 'Boom', severity: 'error', kind: 'engineFailed' }), ev(2, { message: 'Careful', severity: 'warn' }), ev(3, { message: 'Fine' })]);
    const items = Array.from(el.querySelectorAll('li'));
    expect(items.map((i) => i.getAttribute('data-tone'))).toEqual(['bad', 'warn', 'info']);
    expect(items.map((i) => i.querySelector('.msg')!.textContent)).toEqual(['Boom', 'Careful', 'Fine']);
    expect(items[0]!.querySelector('time')!.textContent).toBe('2 min ago');
    expect(items[0]!.querySelector('mat-icon')!.getAttribute('data-mat-icon-name')).toBe('error');
    expect(items[2]!.querySelector('mat-icon')!.getAttribute('data-mat-icon-name')).toBe('models');
    expect(el.querySelector('pw-empty-state')).toBeNull();
  });

  it('shows the reason an action happened, when there is one', async () => {
    const { el } = await mount([ev(1, { reason: 'idle', kind: 'modelUnloaded' }), ev(2)]);
    const items = el.querySelectorAll('li');
    expect(items[0]!.querySelector('.reason')!.textContent).toMatch(/\S/);
    expect(items[1]!.querySelector('.reason')).toBeNull();
  });

  it('shows only the newest events up to the limit', async () => {
    const { el } = await mount([ev(1, { message: 'one' }), ev(2, { message: 'two' }), ev(3, { message: 'three' })], { limit: 2 });
    expect(Array.from(el.querySelectorAll('.msg')).map((m) => m.textContent)).toEqual(['one', 'two']);
  });

  it('renders the mock events by default', async () => {
    TestBed.configureTestingModule({ imports: [EventsFeed], providers: [{ provide: BACKEND, useValue: new MockBackend() }] });
    registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    await TestBed.inject(LiveStore).load();
    const fixture = TestBed.createComponent(EventsFeed);
    fixture.componentRef.setInput('now', Date.now());
    await settle(fixture);
    const n = (fixture.nativeElement as HTMLElement).querySelectorAll('li').length;
    expect(n).toBeGreaterThan(0);
    expect(n).toBeLessThanOrEqual(30);
  });
});
