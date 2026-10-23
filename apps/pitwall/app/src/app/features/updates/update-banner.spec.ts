import { TestBed } from '@angular/core/testing';
import { patchState } from '@ngrx/signals';
import { MockBackend } from '../../core/mock-backend';
import { Confirm } from '../../shared/ui/confirm';
import { button, render, settle } from '../../../testing/render';
import { UpdateBanner } from './update-banner';
import { UpdatesStore } from './updates.store';

describe('UpdateBanner', () => {
  it('shows nothing when there is nothing on offer', async () => {
    const { el } = await render(UpdateBanner, new MockBackend());
    expect(el.querySelector('.banner')).toBeNull();
  });

  it('lists each update with what it is, from where, and the three actions', async () => {
    const { el } = await render(UpdateBanner, new MockBackend({ modelUpdates: true }));
    const rows = el.querySelectorAll('li');
    expect(rows).toHaveLength(2);
    expect(el.querySelector('header')!.textContent).toContain('2 updates available for models you downloaded');
    expect(el.textContent).toContain('nothing downloads until you choose');
    expect(rows[0]!.textContent).toContain('Update available: newer revision');
    expect(rows[0]!.textContent).toContain('Qwen3.5-9B-Q4_K_M.gguf');
    expect(rows[0]!.textContent).toContain('unsloth/Qwen3.5-9B-GGUF');
    expect(rows[1]!.textContent).toContain('Update available: better quantisation');
    expect(button(rows[0] as HTMLElement, 'Get Qwen3.5-9B-Q4_K_M.gguf')).toBeTruthy();
    expect(button(rows[0] as HTMLElement, 'Remind me about qwen3.5-9b-q4_k_m in a week')).toBeTruthy();
    expect(button(rows[0] as HTMLElement, 'Dismiss the update for qwen3.5-9b-q4_k_m')).toBeTruthy();
  });

  it('dismisses an update and snoozes another', async () => {
    const { el, fixture } = await render(UpdateBanner, new MockBackend({ modelUpdates: true }));
    button(el, 'Dismiss the update for qwen3.5-9b-q4_k_m').click();
    await settle(fixture);
    expect(el.querySelectorAll('li')).toHaveLength(1);
    expect(el.querySelector('header')!.textContent).toContain('1 update available');
    button(el, 'Remind me about qwen3.5-4b-q4_k_m in a week').click();
    await settle(fixture);
    expect(el.querySelector('.banner')).toBeNull();
  });

  it('"Get it" confirms with the host before anything downloads', async () => {
    const backend = new MockBackend({ modelUpdates: true });
    const spy = vi.spyOn(backend, 'downloadModel');
    const ask = vi.fn().mockResolvedValue(false);
    TestBed.configureTestingModule({ providers: [{ provide: Confirm, useValue: { ask } }] });
    const { el, fixture } = await render(UpdateBanner, backend);
    button(el, 'Get Qwen3.5-9B-Q4_K_M.gguf').click();
    await settle(fixture);
    expect(ask).toHaveBeenCalledOnce();
    expect(ask.mock.calls[0]![0].message).toContain('huggingface.co');
    expect(spy).not.toHaveBeenCalled();
    ask.mockResolvedValue(true);
    button(el, 'Get Qwen3.5-9B-Q4_K_M.gguf').click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('unsloth/Qwen3.5-9B-GGUF', 'Qwen3.5-9B-Q4_K_M.gguf', null);
    expect(el.querySelectorAll('li')).toHaveLength(1);
  });

  it('collapses a long list behind Show all', async () => {
    const backend = new MockBackend({ modelUpdates: true });
    const { el, fixture } = await render(UpdateBanner, backend);
    const store = TestBed.inject(UpdatesStore);
    const base = store.notices()[0]!;
    const many = Array.from({ length: 5 }, (_, i) => ({ ...base, id: `m${i}#betterQuant`, modelId: `m${i}` }));
    patchState(store as never, { notices: many });
    await settle(fixture);
    expect(el.querySelectorAll('li')).toHaveLength(3);
    button(el, 'Show all 5').click();
    await settle(fixture);
    expect(el.querySelectorAll('li')).toHaveLength(5);
    expect(el.textContent).not.toContain('Show all');
  });

  it('as a summary (dashboard) it is one line that links to Models', async () => {
    const { el, fixture } = await render(UpdateBanner, new MockBackend({ modelUpdates: true }));
    fixture.componentRef.setInput('mode', 'summary');
    await settle(fixture);
    expect(el.querySelectorAll('li')).toHaveLength(0);
    expect(el.querySelector('.summary')!.textContent).toContain('2 model updates are available');
    expect(el.querySelector('a')!.getAttribute('href')).toBe('/models');
  });
});
