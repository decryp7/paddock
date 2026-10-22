import { TestBed } from '@angular/core/testing';
import type { EngineIssue } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { render, settle } from '../../../testing/render';
import { EngineIssues, restartText } from './engine-issues';

const restarting: EngineIssue = { modelId: 'qwen', state: 'restarting', crashes: 1, restartAt: 5000, message: 'Pitwall is restarting it.' };
const failed: EngineIssue = { modelId: 'llama', state: 'failed', crashes: 6, restartAt: null, message: 'Check the model file, then press Retry.' };

describe('restartText', () => {
  it('counts down to a planned restart, says restarting when it is under way, and is empty for failed models', () => {
    expect(restartText(restarting, 2000)).toBe('Restarts in 0:03');
    expect(restartText({ ...restarting, restartAt: null }, 2000)).toBe('Restarting now');
    expect(restartText(failed, 2000)).toBeNull();
  });
});

describe('EngineIssues', () => {
  it('shows nothing while all is well', async () => {
    const { el } = await render(EngineIssues, new MockBackend());
    expect(el.querySelector('.issue')).toBeNull();
  });

  it('lists restarting and failed models, Retry loads the failed one', async () => {
    const backend = new MockBackend({ crashed: true });
    const { fixture, el } = await render(EngineIssues, backend);
    fixture.componentRef.setInput('now', 0);
    const live = TestBed.inject(LiveStore);
    await live.load();
    live.connect();
    await settle(fixture);
    expect(el.querySelectorAll('.issue').length).toBe(1);
    expect(el.querySelector('.issue')?.getAttribute('data-state')).toBe('failed');
    expect(el.textContent).toContain('keeps crashing');
    const spy = vi.spyOn(backend, 'loadModel');
    el.querySelector<HTMLButtonElement>('button')!.click();
    await settle(fixture);
    expect(spy).toHaveBeenCalled();
    expect(el.querySelector('.issue')).toBeNull();
  });
});
