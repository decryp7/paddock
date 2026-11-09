import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { LocalModel } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { registerIcons } from '../../shared/icons';
import { ModelsStore } from './models.store';
import { SlotsPanel } from './slots-panel';
import { FIT_DELAY_MS } from './slots.store';

const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

async function render(id = 'gemma-4-12b-it-q4_k_m', backend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [ModelsStore, { provide: BACKEND, useValue: backend }] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const store = TestBed.inject(ModelsStore);
  await store.load();
  const model = store.library().find((m) => m.id === id) as LocalModel;
  const fixture = TestBed.createComponent(SlotsPanel);
  fixture.componentRef.setInput('model', model);
  const closed = vi.fn();
  fixture.componentInstance.closed.subscribe(closed);
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    await tick();
    fixture.detectChanges();
    await tick();
    fixture.detectChanges();
  };
  await settle();
  return { fixture, el, store, settle, closed, backend };
}

const pick = (el: HTMLElement, n: number) => Array.from(el.querySelectorAll('[role="radio"]')).find((b) => b.querySelector('b')?.textContent === String(n)) as HTMLButtonElement;

describe('SlotsPanel', () => {
  it('shows what each step adds and the engine fit table in words', async () => {
    const { el } = await render();
    expect(el.textContent).toContain('Parallel slots · Gemma 4 12B');
    expect(el.querySelectorAll('[role="radio"]').length).toBe(4);
    expect(pick(el, 1).textContent).toContain('baseline');
    expect(pick(el, 4).textContent).toMatch(/adds ~\d+\.\d GB/);
    expect(pick(el, 2).getAttribute('aria-checked')).toBe('true');
    const rows = el.querySelectorAll('table.choices tbody tr');
    expect(rows.length).toBe(4);
    expect(rows[1]!.classList.contains('selected')).toBe(true);
    expect(el.querySelector('.answer')?.textContent).toMatch(/Fits|Tight|Won't fit/);
    expect(el.querySelector('.sum')?.textContent).toContain('requests at once');
    expect(el.textContent).toContain('applies the next time the model loads');
  });

  it('asks the engine again when the sliders move and marks a count that does not fit', async () => {
    const { fixture, el, backend } = await render();
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      const spy = vi.spyOn(backend, 'modelFit');
      const range = el.querySelectorAll('input[type="range"]');
      const slots = range[0] as HTMLInputElement;
      slots.value = '12';
      slots.dispatchEvent(new Event('input'));
      const ctx = range[1] as HTMLInputElement;
      ctx.value = '4';
      ctx.dispatchEvent(new Event('input'));
      fixture.detectChanges();
      expect(el.textContent).toContain('12 slots');
      expect(el.textContent).toContain('32,768 tokens');
      vi.advanceTimersByTime(FIT_DELAY_MS + 1);
      expect(spy).toHaveBeenCalledTimes(1);
      expect(spy).toHaveBeenCalledWith('gemma-4-12b-it-q4_k_m', 32768, 12);
    } finally {
      vi.useRealTimers();
    }
  });

  it('saves the chosen count and closes; unchanged counts cannot be saved', async () => {
    const { fixture, el, store, settle, closed } = await render();
    const save = () => Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.startsWith('Save')) as HTMLButtonElement;
    expect(save().disabled).toBe(true);
    pick(el, 4).click();
    fixture.detectChanges();
    expect(save().disabled).toBe(false);
    expect(save().textContent).toContain('Save 4 slots');
    save().click();
    await settle();
    expect(closed).toHaveBeenCalled();
    expect(store.library().find((m) => m.id === 'gemma-4-12b-it-q4_k_m')).toMatchObject({ slots: 4, slotsCustom: true });
  });

  it('returns to the default and says a loaded model keeps its count until it loads again', async () => {
    const backend = new MockBackend();
    await backend.setModelSlots('qwen3.5-9b-q4_k_m', 8);
    const { el, store, settle, closed } = await render('qwen3.5-9b-q4_k_m', backend);
    expect(el.querySelector('.note')?.textContent).toContain('is loaded with 2 slots; the new count applies the next time it loads');
    expect(el.querySelector('.note')?.textContent).toContain('not interrupted');
    const def = Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.trim() === 'Use the default') as HTMLButtonElement;
    def.click();
    await settle();
    expect(closed).toHaveBeenCalled();
    expect(store.library().find((m) => m.id === 'qwen3.5-9b-q4_k_m')?.slotsCustom).toBe(false);
  });

  it('shows an error with Retry when the engine cannot answer, and a refused save stays open', async () => {
    const failing = new MockBackend({ failing: ['modelFit', 'setModelSlots'] } as never);
    const { fixture, el, settle, closed } = await render('gemma-4-12b-it-q4_k_m', failing);
    expect(el.querySelector('.err')?.textContent).toContain('not responding');
    expect(Array.from(el.querySelectorAll('button')).some((b) => b.textContent?.trim() === 'Retry')).toBe(true);
    pick(el, 8).click();
    fixture.detectChanges();
    (Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.startsWith('Save')) as HTMLButtonElement).click();
    await settle();
    expect(closed).not.toHaveBeenCalled();
    expect(TestBed.inject(Toasts).toasts().at(-1)?.tone).toBe('bad');
  });

  it('closes from its button', async () => {
    const { el, closed } = await render();
    (el.querySelector('button[aria-label="Close slots"]') as HTMLButtonElement).click();
    expect(closed).toHaveBeenCalled();
  });
});
