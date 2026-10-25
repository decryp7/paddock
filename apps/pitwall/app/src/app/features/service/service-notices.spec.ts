import { MockBackend } from '../../core/mock-backend';
import { button, render, settle } from '../../../testing/render';
import { ServiceNotices } from './service-notices';
import { ServiceStore } from './service.store';
import { TestBed } from '@angular/core/testing';

describe('ServiceNotices', () => {
  async function setup(backend: MockBackend) {
    const r = await render(ServiceNotices, backend);
    await TestBed.inject(ServiceStore).refresh();
    await settle(r.fixture);
    return { ...r, backend };
  }

  it('shows nothing when the service is fine', async () => {
    const { el } = await setup(new MockBackend());
    expect(el.querySelector('.callout')).toBeNull();
  });

  it('shows the repair while it works', async () => {
    const { el } = await setup(new MockBackend({ repairing: true }));
    expect(el.querySelector('[data-testid="service-repair"]')?.textContent).toContain('re-registering');
    expect(el.querySelector('[data-testid="service-disabled"]')).toBeNull();
  });

  it('says the item is switched off and opens Login Items on the button', async () => {
    const backend = new MockBackend({ loginItemOff: true });
    const { el, fixture } = await setup(backend);
    expect(el.querySelector('[data-testid="service-disabled"]')?.textContent).toContain('Login Items');
    await button(el, 'Open Login Items').click();
    await settle(fixture);
    expect(backend.loginItemsOpened).toBe(1);
  });

  it('says the service could not start after the update and repairs it on the button', async () => {
    const backend = new MockBackend({ serverDown: true, startFailed: true });
    const { el, fixture } = await setup(backend);
    expect(el.querySelector('[data-testid="service-start-failed"]')?.textContent).toContain('could not start after the update');
    await button(el, 'Repair').click();
    await settle(fixture);
    expect(backend.repairs).toBe(1);
    expect(el.querySelector('[data-testid="service-start-failed"]')).toBeNull();
  });
});
