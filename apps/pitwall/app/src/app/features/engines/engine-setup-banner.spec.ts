import { TestBed } from '@angular/core/testing';
import type { Download } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { render, settle } from '../../../testing/render';
import { EngineSetupBanner } from './engine-setup-banner';

const download: Download = { id: 'eng-0', kind: 'engine', name: 'llama.cpp Metal (b11669)', host: 'github.com', bytesDone: 50e6, bytesTotal: 100e6, speedBps: 1e6, state: 'downloading', error: null, startedAt: 0 };

describe('EngineSetupBanner', () => {
  async function open() {
    const backend = new MockBackend();
    const r = await render(EngineSetupBanner, backend);
    const live = TestBed.inject(LiveStore);
    await live.load();
    live.connect();
    await settle(r.fixture);
    return { ...r, backend, live };
  }

  it('shows nothing while nothing is being installed', async () => {
    const { el } = await open();
    expect(el.querySelector('.setup')).toBeNull();
  });

  it('shows the progress and says models wait while llama.cpp installs itself', async () => {
    const { fixture, el, backend } = await open();
    backend.setEngineSetup({ state: 'installing', error: null, download });
    await settle(fixture);
    expect(el.textContent).toContain('Installing llama.cpp…');
    expect(el.textContent).toContain('wait');
    expect(el.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow')).toBe('50');
  });

  it('shows why it failed with Retry, which clears the failure', async () => {
    const { fixture, el, backend } = await open();
    backend.setEngineSetup({ state: 'failed', error: 'The network is unreachable. Then press Retry.', download: null });
    await settle(fixture);
    expect(el.querySelector('.setup')?.getAttribute('data-state')).toBe('failed');
    expect(el.textContent).toContain('network is unreachable');
    el.querySelector<HTMLButtonElement>('button')!.click();
    await settle(fixture);
    expect(el.querySelector('.setup')).toBeNull();
  });
});
