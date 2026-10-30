import { TestBed } from '@angular/core/testing';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import { LiveStore } from '../core/live.store';
import { MockBackend } from '../core/mock-backend';
import { render, settle } from '../../testing/render';
import { MemoryStatusItem, ServingStatusItem } from './status-items';

const text = (el: HTMLElement) => el.textContent!.replace(/\s+/g, ' ').trim();

describe('ServingStatusItem', () => {
  it('says Connecting until the server answers, then the serving state with a dot and the port in the tooltip', async () => {
    const { fixture, el } = await render(ServingStatusItem, new MockBackend());
    expect(text(el)).toBe('Connecting…');
    await TestBed.inject(LiveStore).load();
    await settle(fixture);
    const state = el.querySelector<HTMLElement>('[data-testid="serving-status"]')!;
    expect(text(el)).toBe('Serving');
    expect(state.dataset['state']).toBe('serving');
    expect(state.querySelector('.dot')).not.toBeNull();
  });

  it('says Unreachable when the server cannot be asked', async () => {
    const { fixture, el } = await render(ServingStatusItem, new MockBackend({ failing: ['overview'] }));
    await TestBed.inject(LiveStore).load();
    await settle(fixture);
    expect(text(el)).toBe('Unreachable');
    expect(el.querySelector<HTMLElement>('[data-state]')!.dataset['state']).toBe('down');
  });
});

describe('MemoryStatusItem', () => {
  it('shows RAM and VRAM in use, and a word only past 85 %', async () => {
    const { fixture, el } = await render(MemoryStatusItem, new MockBackend());
    expect(el.querySelector('[data-testid="memory-status"]')).toBeNull();
    const live = TestBed.inject(LiveStore);
    await live.load();
    await settle(fixture);
    const item = () => el.querySelector<HTMLElement>('[data-testid="memory-status"]')!;
    expect(item().getAttribute('aria-label')).toMatch(/^Memory: [\d.]+ \w+ of [\d.]+ \w+ RAM in use, .+ VRAM$/);
    const m = live.memory()!;
    patchState(live as unknown as WritableStateSource<object>, { overview: { ...live.overview()!, memory: { ...m, ramAvailable: m.ramTotal * 0.02, gpus: [] } } });
    await settle(fixture);
    expect(text(el)).toContain('Nearly full');
    expect(item().getAttribute('aria-label')).toMatch(/nearly full$/);
    expect(text(el)).not.toContain('VRAM');
    expect(el.querySelector<HTMLElement>('[data-testid="memory-status"]')!.dataset['tone']).toBe('bad');
  });
});
