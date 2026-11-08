import { TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Logger } from '../../core/logger';
import { MockAiBackend, MockAiOptions } from '../../core/mock/mock-ai-backend';
import { MemoryLogger, settle } from '../../../testing/helpers';
import { gatedPairingClock } from '../../../testing/pitwall-clock';
import { AiBackend, AiError } from '../ai-backend';
import { AiStatusStore } from '../ai-status.store';
import { AiSettingsExtras } from './ai-settings-extras';

function setup(options: MockAiOptions = {}) {
  const backend = new MockAiBackend({ stepMs: 0, ...options });
  TestBed.configureTestingModule({
    providers: [
      { provide: AiBackend, useValue: backend },
      { provide: Logger, useClass: MemoryLogger },
    ],
  });
  const fixture = TestBed.createComponent(AiSettingsExtras);
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  const q = (id: string) => el.querySelector<HTMLElement>(`[data-testid="${id}"]`);
  const button = (text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent!.trim() === text);
  const flush = async () => {
    await settle();
    fixture.detectChanges();
  };
  return { backend, fixture, el, q, button, flush };
}

const text = (el: HTMLElement) => el.textContent!.replace(/\s+/g, ' ');

describe('AiSettingsExtras', () => {
  afterEach(() => TestBed.resetTestingModule());

  it('shows the engine and model status', async () => {
    const { q, flush } = setup();
    await flush();
    expect(q('ai-settings-status')!.textContent).toContain('Model: qwen3-8b-q4km');
    const store = TestBed.inject(AiStatusStore);
    store.setStatus({ ...store.status()!, state: 'noModel', model: null });
    await flush();
    expect(q('ai-settings-status')!.textContent).toContain('Pitwall serves no model for Debrief yet');
    store.setStatus(null as never);
    await flush();
    expect(q('ai-settings-unavailable')).not.toBeNull();
  });

  it('explains when AI is unavailable', async () => {
    const { q, flush } = setup({ mode: 'off' });
    await flush();
    expect(q('ai-settings-unavailable')!.textContent).toContain('not available');
  });

  describe('Pitwall', () => {

    it('says Pitwall is not installed when it is absent, with no Pitwall buttons', async () => {
      const { q, el, flush } = setup({ pitwall: 'absent' });
      await flush();
      expect(q('ai-engine-status')!.textContent).toContain('Engine: Pitwall. It cannot serve Debrief right now.');
      expect(text(el)).toContain('Pitwall is not installed on this computer');
      for (const id of ['ai-pitwall-connect', 'ai-pitwall-cancel', 'ai-pitwall-forget']) expect(q(id)).toBeNull();
    });

    it('offers to connect when Pitwall runs unpaired, shows the code while waiting and ends in Pitwall', async () => {
      const gate = gatedPairingClock();
      const { q, el, flush, backend } = setup({ pitwall: 'running', stepMs: 1, clock: gate.clock });
      await flush();
      expect(text(el)).toContain('Pitwall is running on this computer. Connect Debrief to it');
      const connect = vi.spyOn(backend, 'pitwallConnect');
      q('ai-pitwall-connect')!.click();
      await flush();
      expect(connect).toHaveBeenCalled();
      expect(q('ai-pitwall-code')!.textContent).toBe('482 913');
      expect(q('ai-pitwall-connect')).toBeNull();
      expect(q('ai-pitwall-cancel')).not.toBeNull();
      gate.approve();
      for (let i = 0; i < 50 && !q('ai-pitwall-forget'); i++) await flush();
      expect(q('ai-engine-status')!.textContent).toBe('Engine: Pitwall 1.0.4 · model qwen3-8b-q4km');
      expect(q('ai-pitwall-code')).toBeNull();
      expect(q('ai-pitwall-connect')).toBeNull();
    });

    it('cancels a waiting pairing', async () => {
      const { q, flush, backend } = setup({ pitwall: 'running', stepMs: 1, clock: gatedPairingClock().clock });
      await flush();
      q('ai-pitwall-connect')!.click();
      await flush();
      const cancel = vi.spyOn(backend, 'pitwallCancel');
      q('ai-pitwall-cancel')!.click();
      await flush();
      expect(cancel).toHaveBeenCalled();
      expect(q('ai-pitwall-code')).toBeNull();
      expect(q('ai-pitwall-connect')).not.toBeNull();
    });

    it('forgets a paired Pitwall and shows the denial as the problem', async () => {
      const { q, flush, backend } = setup({ pitwall: 'paired' });
      await flush();
      expect(q('ai-engine-status')!.textContent).toContain('Engine: Pitwall 1.0.4');
      const forget = vi.spyOn(backend, 'pitwallForget');
      q('ai-pitwall-forget')!.click();
      await flush();
      expect(forget).toHaveBeenCalled();
      expect(q('ai-pitwall-forget')).toBeNull();
      expect(q('ai-engine-status')!.textContent).toContain('Engine: Pitwall. It cannot serve Debrief right now.');
    });

    it('shows what Pitwall reports as the problem', async () => {
      const { el, flush } = setup({ pitwall: 'installed' });
      await flush();
      const store = TestBed.inject(AiStatusStore);
      const s = store.status()!;
      store.setStatus({ ...s, pitwall: { ...s.pitwall, problem: 'Pitwall stopped.' } });
      await flush();
      expect(text(el)).toContain('Pitwall is installed but not running. Open Pitwall to use it.');
      expect(text(el)).toContain('Pitwall stopped.');
    });

    it('shows a rejected command inline', async () => {
      const { q, flush, backend } = setup({ pitwall: 'running' });
      await flush();
      vi.spyOn(backend, 'pitwallConnect').mockRejectedValue(new AiError('engine', 'Pitwall did not answer.'));
      q('ai-pitwall-connect')!.click();
      await flush();
      expect(q('ai-engine-error')!.textContent).toBe('Pitwall did not answer.');
      expect(q('ai-engine-error')!.getAttribute('role')).toBe('alert');
    });
  });
});
