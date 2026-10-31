import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { registerIcons } from '../../shared/icons';
import { dialogsAnswering } from '../../../testing/render';
import { EnginesPage } from './engines-page';
import { EnginesStore } from './engines.store';

const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

describe('Engine pins in the store', () => {
  function setup(backend: PitwallBackend = new MockBackend()) {
    TestBed.configureTestingModule({ providers: [EnginesStore, { provide: BACKEND, useValue: backend }] });
    return { store: TestBed.inject(EnginesStore), toasts: TestBed.inject(Toasts) };
  }

  it('loads the pins and what each build supports', async () => {
    const { store } = setup();
    await store.load();
    expect(store.pins()).toMatchObject({ global: null, active: 'cuda-13', rollbackTo: { buildId: 'vulkan' } });
    expect(store.pinnedGlobal()).toBeNull();
    expect(Object.keys(store.capabilities()).sort()).toEqual(['cpu', 'cuda-13', 'vulkan']);
    expect(store.capabilities()['cpu']!.find((c) => c.capability === 'vision')?.status).toBe('needsBuild');
  });

  it('keeps the page working when the pins cannot be read', async () => {
    const { store } = setup(new MockBackend({ failing: ['enginePins'] }));
    await store.load();
    expect(store.loaded()).toBe(true);
    expect(store.pins()).toBeNull();
    expect(store.capabilities()).toEqual({});
  });

  it('pins a build and unpins it again, toasting what happens to loaded models', async () => {
    const { store, toasts } = setup();
    await store.load();
    const vulkan = store.llama()!.builds.find((b) => b.id === 'vulkan')!;
    expect(await store.togglePin(vulkan)).toBe(true);
    expect(store.pinnedGlobal()).toBe('vulkan');
    expect(toasts.toasts().at(-1)?.message).toContain('vulkan is pinned for all models');
    expect(toasts.toasts().at(-1)?.message).toContain('until they are loaded again');
    expect(await store.togglePin(vulkan)).toBe(true);
    expect(store.pinnedGlobal()).toBeNull();
  });

  it('toasts the typed reason when a pin is refused and keeps the last good state', async () => {
    const { store, toasts } = setup();
    await store.load();
    const cpu = store.llama()!.builds.find((b) => b.id === 'cpu')!;
    expect(await store.togglePin(cpu)).toBe(false);
    expect(toasts.toasts().at(-1)?.message).toContain('self-test');
    expect(store.pinnedGlobal()).toBeNull();
  });
});

describe('Engine pins on the Engines page', () => {
  async function render(backend: PitwallBackend = new MockBackend(), confirm = true) {
    const dialogs = dialogsAnswering(confirm);
    TestBed.configureTestingModule({
      imports: [EnginesPage],
      providers: [
        { provide: BACKEND, useValue: backend },
        dialogs,
      ],
    });
    registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    const fixture = TestBed.createComponent(EnginesPage);
    fixture.detectChanges();
    await tick();
    fixture.detectChanges();
    const settle = async () => {
      await tick();
      fixture.detectChanges();
      await tick();
      fixture.detectChanges();
    };
    const el = fixture.nativeElement as HTMLElement;
    const btn = (label: string) => el.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
    return { el, settle, btn, opened: dialogs.opened };
  }

  it('offers a pin button per build, with the unpinned ones saying Pin', async () => {
    const { el, btn } = await render();
    // The pin is an icon on the row (shown on hover and focus), not a labelled button on every row.
    expect(el.textContent).not.toContain('Version pin');
    const b = btn('Pin Vulkan b11669 for all models');
    expect(b.getAttribute('aria-pressed')).toBe('false');
    expect(b.textContent!.trim()).toBe('');
    expect(b.classList.contains('reveal')).toBe(true);
    // A build that did not pass its self-test cannot be pinned.
    expect(btn('Pin CPU (AVX2) b4300 for all models').disabled).toBe(true);
  });

  it('pins a build: accent and words, then unpins it', async () => {
    const { el, btn, settle } = await render();
    btn('Pin Vulkan b11669 for all models').click();
    await settle();
    const pinned = btn('Unpin Vulkan b11669 for all models');
    expect(pinned.getAttribute('aria-pressed')).toBe('true');
    // Pinned stays visible on the row, with a name for screen readers.
    expect(pinned.classList.contains('on')).toBe(true);
    expect(el.querySelector('tbody mat-icon.pinned')?.getAttribute('aria-label')).toBe('Pinned for all models');
    expect(el.querySelector('.pinned-note')?.textContent).toContain('Pinned to Vulkan b11669 for all models');
    pinned.click();
    await settle();
    expect(el.querySelector('.pinned-note')).toBeNull();
    expect(btn('Pin Vulkan b11669 for all models')).not.toBeNull();
  });

  it('lists what a build supports in words, with upgrade hints', async () => {
    const { el, btn, settle } = await render();
    expect(el.querySelector('.caps-row')).toBeNull();
    const toggle = btn('What CPU (AVX2) b4300 supports');
    toggle.click();
    await settle();
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    const list = el.querySelector('ul.caps')!;
    expect(list.getAttribute('aria-label')).toBe('Features of CPU (AVX2) b4300');
    expect(list.textContent).toContain('Needs b5600');
    expect(list.textContent).toContain('Image input');
    expect(list.textContent).toContain('Install llama.cpp b5600 or newer on the llama.cpp page');
    expect(list.textContent).toContain('Supported');
    toggle.click();
    await settle();
    expect(el.querySelector('.caps-row')).toBeNull();
  });

  it('says the engine did not report features when the capability call fails', async () => {
    const { el, btn, settle } = await render(new MockBackend({ failing: ['buildCapabilities'] }));
    btn('What Vulkan b11669 supports').click();
    await settle();
    expect(el.querySelector('.caps-row')?.textContent).toContain('did not report');
  });

  it('names the roll back target and what pinning it means before doing it', async () => {
    const { el, settle, opened } = await render();
    const rollback = Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.includes('Roll back to Vulkan b11669'))!;
    rollback.click();
    await settle();
    expect(opened.at(-1)).toMatchObject({ id: 'pd.confirm', inputs: { heading: 'Roll back to Vulkan b11669?', message: expect.stringContaining('pinned for all models'), confirm: { label: 'Roll back' } } });
    expect(el.querySelector('.pinned-note')?.textContent).toContain('Vulkan b11669');
  });

  it('shows why roll back is unavailable instead of a dead button', async () => {
    const backend = new MockBackend();
    await backend.removeBuild('vulkan', 'b11669');
    const { el } = await render(backend);
    expect(el.querySelector('[data-test="rollback-blocked"]')?.textContent).toContain('no longer installed');
    expect(Array.from(el.querySelectorAll('button')).some((b) => b.textContent?.includes('Roll back to'))).toBe(false);
  });

  it('shows pins that cannot be used with what to do', async () => {
    const mock = new MockBackend();
    const backend = new Proxy(mock, {
      get: (t, k) =>
        k === 'enginePins'
          ? async () => ({ ...(await mock.enginePins()), problems: ['The engine build metal is pinned for all models but is not installed. Install it on the llama.cpp page, or remove the pin.'] })
          : Reflect.get(t, k, t),
    }) as PitwallBackend;
    const { el } = await render(backend);
    expect(el.querySelector('.pin-problem')?.textContent).toContain('Install it on the llama.cpp page');
  });

  it('has no pin controls when the engine cannot report pins', async () => {
    const { el } = await render(new MockBackend({ failing: ['enginePins'] }));
    expect(el.textContent).not.toContain('Version pin');
    expect(el.querySelector('button[aria-label^="Pin "]')).toBeNull();
    expect(el.querySelector('.pinned-note')).toBeNull();
  });
});
