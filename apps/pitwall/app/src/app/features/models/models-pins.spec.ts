import { pickOption } from '../../../testing/render';
import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { registerIcons } from '../../shared/icons';
import { Confirm } from '../../shared/ui/confirm';
import { ModelsPage } from './models-page';
import { ModelsStore } from './models.store';


const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

async function render(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({
    imports: [ModelsPage],
    providers: [
      { provide: BACKEND, useValue: backend },
      { provide: Confirm, useValue: { ask: () => Promise.resolve(true) } },
    ],
  });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  await TestBed.inject(LiveStore).load();
  const fixture = TestBed.createComponent(ModelsPage);
  fixture.detectChanges();
  await tick();
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    await tick();
    fixture.detectChanges();
    await tick();
    fixture.detectChanges();
  };
  /** Opens the first model's details (the build setting lives there). */
  const open = async () => {
    el.querySelector<HTMLElement>('pw-library-tab tbody tr')!.click();
    await settle();
  };
  return { el, settle, open, toasts: TestBed.inject(Toasts) };
}

describe('Per-model engine pins in the library', () => {
  it('offers a build menu per model with only builds that passed their self-test', async () => {
    const { el, open, settle } = await render();
    await open();
    const select = el.querySelector<HTMLElement>('mat-select[id^="pin-"]')!;
    expect(select).not.toBeNull();
    select.querySelector<HTMLElement>('.mat-mdc-select-trigger')!.click();
    await settle();
    const options = Array.from(document.querySelectorAll('.mat-mdc-option')).map((o) => o.textContent?.trim());
    expect(options).toEqual(['Follows global choice', 'Pin to CUDA 13 (NVIDIA) b11669', 'Pin to Vulkan b11669']);
    expect(el.querySelector('.pinned-build')).toBeNull();
  });

  it('pins a model, shows "pinned to" in words, and unpins it', async () => {
    const backend = new MockBackend();
    const { el, settle, toasts, open } = await render(backend);
    await open();
    const select = el.querySelector<HTMLElement>('mat-select[id^="pin-"]')!;
    const id = select.id.replace('pin-', '');
    await pickOption(select, 'Pin to Vulkan b11669', settle);
    expect((await backend.enginePins()).models).toEqual([{ modelId: id, buildId: 'vulkan', tag: 'b11669' }]);
    expect(el.querySelector('.pinned-build')?.textContent).toContain('Pinned to vulkan b11669');
    expect(toasts.toasts().at(-1)?.message).toContain(`${id} is pinned to vulkan`);
    await pickOption(el.querySelector<HTMLElement>(`#pin-${CSS.escape(id)}`)!, 'Follows global choice', settle);
    expect((await backend.enginePins()).models).toEqual([]);
    expect(el.querySelector('.pinned-build')).toBeNull();
  });

  it('keeps the library working and hides the menu when the pins cannot be read', async () => {
    const { el, open } = await render(new MockBackend({ failing: ['enginePins'] }));
    await open();
    expect(el.querySelector('[id^="pin-"]')).toBeNull();
    expect(el.querySelector('table')).not.toBeNull();
  });

  it('toasts the reason and keeps the state when a pin is refused', async () => {
    const mock = new MockBackend();
    const backend = new Proxy(mock, {
      get: (t, k) => (k === 'setModelPin' ? () => Promise.reject({ code: 'notFound', message: 'The engine build metal is pinned for x but is not installed. Install it on the llama.cpp page, or remove the pin.' }) : Reflect.get(t, k, t)),
    }) as PitwallBackend;
    const { el, settle, toasts, open } = await render(backend);
    await open();
    await pickOption(el.querySelector<HTMLElement>('mat-select[id^="pin-"]')!, 'Pin to Vulkan b11669', settle);
    expect(toasts.toasts().at(-1)?.message).toContain('Install it on the llama.cpp page');
    expect(el.querySelector('.pinned-build')).toBeNull();
  });

  it('exposes the pinnable builds and a model\'s pin from the store', async () => {
    TestBed.configureTestingModule({ providers: [ModelsStore, { provide: BACKEND, useValue: new MockBackend() }] });
    const store = TestBed.inject(ModelsStore);
    await store.loadBuilds();
    expect(store.pinnableBuilds().map((b) => b.id)).toEqual(['cuda-13', 'vulkan']);
    expect(store.pinOf()('anything')).toBeNull();
  });
});
