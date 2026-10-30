import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { provideRouter } from '@angular/router';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import type { Overview } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { settle } from '../../../testing/render';

import { WINDOW_CONTROL, type WindowControl } from '../../core/window-control';
import { TrayPopover } from './tray-popover';

const GiB = 1024 ** 3;

function fakeWindows() {
  const openMain = vi.fn().mockResolvedValue(undefined);
  const windows: WindowControl = { openMain, onNavigate: () => () => undefined };
  return { windows, openMain };
}

async function mount(opts: { backend?: MockBackend; patch?: (o: Overview) => Overview; load?: boolean } = {}) {
  const { windows, openMain } = fakeWindows();
  TestBed.configureTestingModule({ imports: [TrayPopover], providers: [{ provide: BACKEND, useValue: opts.backend ?? new MockBackend() }, { provide: WINDOW_CONTROL, useValue: windows }, provideRouter([])] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  if (opts.load !== false) await live.load();
  if (opts.patch) patchState(live as unknown as WritableStateSource<object>, { overview: opts.patch(live.overview()!) });
  const fixture = TestBed.createComponent(TrayPopover);
  await settle(fixture);
  const el = fixture.nativeElement as HTMLElement;
  const btn = (t: string) => Array.from(el.querySelectorAll('button')).find((b) => b.textContent!.includes(t) || b.getAttribute('aria-label') === t) as HTMLButtonElement;
  return { fixture, el, live, openMain, btn };
}

/** Text without whitespace: inline elements are glued together in the DOM, so compare compactly. */
const text = (e: Element) => e.textContent!.replace(/\s+/g, '');

describe('TrayPopover', () => {
  it('shows the server state, memory, throughput and the loaded models', async () => {
    const { el, live } = await mount();
    const port = live.server()!.port;
    expect(text(el.querySelector('.state')!)).toBe('Servingon:' + port);
    expect(el.querySelector('.state')!.getAttribute('data-state')).toBe('serving');
    expect(text(el.querySelector('.mem .row')!)).toMatch(/^RAM\d+\.\dGB\/32GB(Elevated|Critical)?$/);
    expect(text(el.querySelectorAll('.mem .row')[1]!)).toMatch(/^VRAM[\d.]+GB\/12GB\d+%busy$/);
    const stats = Array.from(el.querySelectorAll('.stats div')).map(text);
    expect(stats).toEqual(['58.4tok/s', '2running', '3waiting']);
    // The pairing banner leaves room for two rows; the rest is summarised instead of cut off.
    expect(el.querySelectorAll('pw-hot-models li.row').length).toBe(2);
    expect(el.querySelector('pw-hot-models .more')!.textContent).toContain('+1 more loaded');
    expect(el.querySelector('pw-hot-models .actions')).toBeNull();
  });

  it('shows the connecting state before the first load', async () => {
    const { el } = await mount({ load: false });
    expect(text(el.querySelector('.state')!)).toBe('Connecting…');
    expect(el.querySelector('.mem')).toBeNull();
    expect(text(el.querySelector('.stats')!)).toBe('0.0tok/s0running0waiting');
  });

  it('says the server is unreachable when loading failed', async () => {
    const { el, btn } = await mount({ backend: new MockBackend({ failing: ['overview'] }) });
    expect(text(el.querySelector('.state')!)).toBe('Serverunreachable');
    expect(el.querySelector('.state')!.getAttribute('data-state')).toBe('down');
    expect(btn('Pause serving').disabled).toBe(true);
    expect(btn('Release memory').disabled).toBe(true);
  });

  it('colours the RAM meter by use', async () => {
    const level = async (avail: number) => {
      TestBed.resetTestingModule();
      const { el } = await mount({ patch: (o) => ({ ...o, memory: { ...o.memory, ramTotal: 100 * GiB, ramAvailable: avail * GiB } }) });
      return el.querySelector('.mem .meter')!.getAttribute('data-tone');
    };
    expect(await level(60)).toBe('neutral');
    expect(await level(25)).toBe('neutral');
    expect(await level(10)).toBe('warn');
    expect(await level(3)).toBe('bad');
  });

  it('hides VRAM when the GPU does not report it, and shows no idle percentage when unknown', async () => {
    const { el } = await mount({ patch: (o) => ({ ...o, memory: { ...o.memory, gpus: [{ ...o.memory.gpus[0]!, vramTotal: null }] } }) });
    expect(el.querySelectorAll('.mem .row').length).toBe(1);
    TestBed.resetTestingModule();
    const unknown = await mount({ patch: (o) => ({ ...o, memory: { ...o.memory, gpus: [{ ...o.memory.gpus[0]!, utilisation: null }] } }) });
    expect(text(unknown.el.querySelectorAll('.mem .row')[1]!)).toMatch(/^VRAM[\d.]+GB\/12GB$/);
  });

  it('shows Paused, zero throughput and Resume while serving is paused, and resumes', async () => {
    const backend = new MockBackend();
    const spy = vi.spyOn(backend, 'setServing');
    const { fixture, el, btn } = await mount({ backend, patch: (o) => ({ ...o, server: { ...o.server, serving: 'paused' } }) });
    expect(text(el.querySelector('.state')!)).toMatch(/^Pausedon:\d+$/);
    expect(text(el.querySelector('.stats div')!)).toBe('0.0tok/s');
    // Open Pitwall is the only filled button, paused or not; Resume sits beside Release as an equal secondary.
    expect(Array.from(el.querySelectorAll('button.mat-mdc-unelevated-button')).map((b) => b.textContent!.trim())).toEqual(['Open Pitwall']);
    expect(btn('Resume').classList.contains('mat-mdc-unelevated-button')).toBe(false);
    expect(Array.from(el.querySelectorAll('button')).some((b) => b.textContent!.includes('Pause serving'))).toBe(false);
    btn('Resume').click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith(false);
  });

  it('pauses serving through the backend', async () => {
    const backend = new MockBackend();
    const spy = vi.spyOn(backend, 'setServing');
    const { fixture, btn } = await mount({ backend });
    btn('Pause serving').click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith(true);
  });

  it('releases memory through the backend', async () => {
    const backend = new MockBackend();
    const spy = vi.spyOn(backend, 'releaseMemory');
    const { fixture, btn } = await mount({ backend });
    btn('Release memory').click();
    await settle(fixture);
    expect(spy).toHaveBeenCalled();
  });

  it('opens the main window from the header and the footer', async () => {
    const { el, openMain } = await mount();
    (el.querySelector('header button[aria-label="Open Pitwall"]') as HTMLButtonElement).click();
    (el.querySelector('footer button.wide') as HTMLButtonElement).click();
    expect(openMain).toHaveBeenNthCalledWith(1, '/');
    expect(openMain).toHaveBeenNthCalledWith(2, '/');
  });

  it('offers a way to review model updates, only when there are some', async () => {
    const none = await mount();
    expect(none.el.textContent).not.toContain('model update');
    TestBed.resetTestingModule();
    const { el, openMain, btn } = await mount({ backend: new MockBackend({ modelUpdates: true }) });
    expect(Array.from(el.querySelectorAll('.pairing')).map(text)).toContain('2modelupdatesareavailable.Review');
    btn('model updates are available').click();
    expect(openMain).toHaveBeenCalledWith('/models');
  });
});
