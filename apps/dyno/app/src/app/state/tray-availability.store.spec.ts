import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { TrayAvailabilityStore, noHostNote } from './tray-availability.store';

const NOTE = "Your desktop isn't showing tray icons. On GNOME, turn on the AppIndicator extension (gnome-shell-extension-appindicator), then Dyno's icons appear.";

function setup(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const mock = new MockBackend({ latencyMs: 0, ...options });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(TrayAvailabilityStore) };
}

describe('TrayAvailabilityStore', () => {
  it('has no note until the engine says the desktop shows no icons', async () => {
    const { store } = setup();
    await vi.waitFor(() => expect(store.loaded()).toBe(true));
    expect(store.note()).toBeNull();
  });

  it('shows the engine note when there is no tray host', async () => {
    const { store } = setup({ tray: 'noHost' });
    await vi.waitFor(() => expect(store.note()).toBe(NOTE));
  });

  it('follows the engine when a host appears later and when it goes', async () => {
    const { mock, store } = setup({ tray: 'noHost' });
    await vi.waitFor(() => expect(store.note()).toBe(NOTE));
    mock.pushTrayAvailability('shown');
    expect(store.note()).toBeNull();
    mock.pushTrayAvailability({ noHost: { hint: NOTE } });
    expect(store.note()).toBe(NOTE);
  });

  it('reads the note out of the availability', () => {
    expect(noHostNote('shown')).toBeNull();
    expect(noHostNote({ noHost: { hint: 'x' } })).toBe('x');
  });
});
