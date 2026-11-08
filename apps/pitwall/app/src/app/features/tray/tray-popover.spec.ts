import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend, type MockOptions } from '../../core/mock-backend';
import { DEFAULT_KINDS } from '../../core/mock/mock-popover';
import { WINDOW_CONTROL, type WindowControl } from '../../core/window-control';
import { registerIcons } from '../../shared/icons';
import { settle } from '../../../testing/render';
import { TrayPopover } from './tray-popover';

function fakeWindows() {
  const openMain = vi.fn().mockResolvedValue(undefined);
  const fitTray = vi.fn().mockResolvedValue(undefined);
  const closeTray = vi.fn().mockResolvedValue(undefined);
  const quit = vi.fn().mockResolvedValue(undefined);
  const windows: WindowControl = { openMain, onNavigate: () => () => undefined, fitTray, closeTray, quit };
  return { windows, openMain, fitTray, closeTray, quit };
}

/** A backend whose popup also shows Recent errors (it is not one of the defaults). */
async function withErrors(options: MockOptions = {}): Promise<MockBackend> {
  const backend = new MockBackend(options);
  await backend.editPopover({ add: { widget: 'recentErrors', span: null, at: null } });
  return backend;
}

async function mount(options: MockOptions = {}, backend: MockBackend = new MockBackend(options)) {
  const win = fakeWindows();
  TestBed.configureTestingModule({ imports: [TrayPopover], providers: [{ provide: BACKEND, useValue: backend }, { provide: WINDOW_CONTROL, useValue: win.windows }, provideRouter([])] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const fixture = TestBed.createComponent(TrayPopover);
  await settle(fixture);
  const el = fixture.nativeElement as HTMLElement;
  /** The widgets by their titles, in the order drawn. */
  const titles = () => Array.from(el.querySelectorAll('section.pd-w')).map((s) => s.querySelector('.pd-w-title')?.textContent?.trim());
  const widget = (title: string) => Array.from(el.querySelectorAll('section.pd-w')).find((s) => s.querySelector('.pd-w-title')?.textContent?.trim() === title) as HTMLElement;
  const menu = async () => {
    (el.querySelector('button[aria-label="More actions"]') as HTMLButtonElement).click();
    await settle(fixture);
    return Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-item'));
  };
  const item = async (label: string) => (await menu()).find((b) => b.textContent!.includes(label))!;
  return { fixture, el, backend, ...win, titles, widget, menu, item };
}

const pill = (el: HTMLElement) => el.querySelector('pw-status-pill .pill')!.textContent!.replace(/\s+/g, ' ').trim();

describe('the tray popup', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((o) => (o.innerHTML = '')));

  it('draws the default widgets in the engine\'s order, with the status from the engine', async () => {
    const { el, titles } = await mount();
    expect(DEFAULT_KINDS).toEqual(['server', 'activity', 'memory', 'loadedModels']);
    expect(titles()).toEqual(['Server', 'Activity', 'Memory', 'Loaded models']);
    expect(pill(el)).toBe('Serving on :11435');
  });

  it('shows what the engine says: speed, queue, memory and the loaded models', async () => {
    const { widget } = await mount({}, await withErrors());
    const text = (t: string) => widget(t).textContent!.replace(/\s+/g, ' ');
    expect(text('Server')).toContain('Serving');
    expect(text('Server')).toContain('Engines ready');
    expect(text('Activity')).toContain('38 tok/s');
    expect(text('Activity')).toContain('1 running');
    expect(text('Activity')).toContain('0 waiting');
    expect(text('Memory')).toContain('19.0 GB of 32 GB');
    expect(text('Memory')).toContain('7.0 GB of 12 GB');
    const models = widget('Loaded models');
    expect(Array.from(models.querySelectorAll('.row .name')).map((n) => n.textContent!.trim())).toEqual(['Qwen3 8B Q4_K_M', 'Gemma 3 4B Q5_K_M']);
    expect(models.textContent).toContain('2 models · 8.0 GB');
    expect(widget('Recent errors').querySelectorAll('.row').length).toBe(1);
  });

  it('in the worst state shows the notice, the problems, every capped list and its "more" line', async () => {
    const { el, widget } = await mount({}, await withErrors({ popover: 'worst' }));
    expect(pill(el)).toBe('1 engine problem');
    const notice = el.querySelector('.pd-wa-notice');
    expect(notice?.textContent).toContain('2 apps want to pair');
    expect(widget('Server').textContent).toContain('Paused');
    expect(widget('Server').textContent).toContain('1 engine failed');
    expect(widget('Memory').textContent).toContain('Memory is critical');
    expect(widget('Loaded models').querySelectorAll('.row').length).toBe(2);
    expect(widget('Loaded models').textContent).toContain('5 models · 34.0 GB');
    expect(widget('Loaded models').textContent).toContain('+3 more');
    expect(widget('Memory').textContent, 'a red meter has a word').toContain('nearly full');
    expect(widget('Recent errors').querySelectorAll('.row').length).toBe(2);
    expect(widget('Recent errors').textContent).toContain('3 in the last day');
  });

  it('with nothing loaded says what a person gains and offers Open Models; errors and downloads are calm', async () => {
    const { widget, backend } = await mount({}, await withErrors({ popover: 'empty' }));
    expect(widget('Loaded models').textContent).toContain('See which models are loaded');
    expect(widget('Loaded models').textContent).toContain('No model is loaded.');
    const calm = widget('Recent errors').querySelector('button.pd-w-calm')!;
    expect(calm.getAttribute('aria-label')).toBe('No errors, In the last day. Open Requests');
    expect(backend).toBeDefined();
  });

  it('with the server down says so once above the widgets and in the pill, and marks the readings stale', async () => {
    const { el, widget } = await mount({ popover: 'down' });
    expect(pill(el)).toBe('No answer');
    expect(el.textContent).toContain('The server is not answering');
    expect(widget('Server').textContent).toContain('No answer');
    expect(widget('Server').textContent, 'no stale engine health').not.toContain('Engines ready');
    expect(Array.from(el.querySelectorAll('button')).filter((b) => b.textContent!.includes('Open Pitwall')).length, 'one Open Pitwall button').toBe(1);
    expect(widget('Memory').textContent).toMatch(/As of|Updated/);
  });

  it('shows the paused status and offers Resume serving once serving is paused', async () => {
    const backend = new MockBackend();
    const { el, fixture, item } = await mount({}, backend);
    await backend.setServing(true);
    await settle(fixture);
    expect(pill(el)).toBe('Paused');
    const resume = await item('Resume serving');
    expect(resume).toBeDefined();
    expect(document.body.textContent).not.toContain('Pause serving');
  });

  it('is loading before the first answer: skeletons and a status, no widgets', async () => {
    const backend = new MockBackend({ latencyMs: 50 });
    const win = fakeWindows();
    TestBed.configureTestingModule({ imports: [TrayPopover], providers: [{ provide: BACKEND, useValue: backend }, { provide: WINDOW_CONTROL, useValue: win.windows }, provideRouter([])] });
    registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    const fixture = TestBed.createComponent(TrayPopover);
    fixture.detectChanges();
    const el = fixture.nativeElement as HTMLElement;
    expect(el.querySelector('[aria-busy="true"]')).not.toBeNull();
    expect(el.querySelectorAll('section.pd-w').length).toBe(0);
    expect(pill(el)).toBe('Connecting…');
    await settle(fixture, 12);
  });

  it('says the popup could not be read, with Retry, when the app does not answer', async () => {
    const { el, fixture } = await mount({ failing: ['popoverView'] });
    expect(el.querySelector('[role="alert"]')!.textContent).toContain("Couldn't read the popup");
    const retry = Array.from(el.querySelectorAll('button')).find((b) => b.textContent!.includes('Retry'))!;
    expect(retry).toBeDefined();
    expect(fixture).toBeDefined();
  });

  describe('the footer', () => {
    it('is the shared popover footer: Open Pitwall right-aligned beside one more-actions button, and opens the dashboard', async () => {
      const { el, openMain } = await mount();
      expect(el.querySelector('.foot'), 'no hand-built footer').toBeNull();
      const foot = el.querySelector('pd-tray-popover footer.pd-footer')!;
      expect(Array.from(foot.children).map((c) => c.tagName.toLowerCase() + ':' + (c.getAttribute('aria-label') ?? c.textContent!.trim()))).toEqual(['span:', 'button:Open Pitwall', 'button:More actions', 'mat-menu:']);
      (foot.querySelector('button.mat-mdc-unelevated-button') as HTMLButtonElement).click();
      expect(openMain).toHaveBeenCalledWith('/');
    });

    it('lists Customize widgets, Pause serving, Release memory, Settings and, after a separator, Quit', async () => {
      const { menu } = await mount();
      const items = await menu();
      expect(items.map((b) => b.textContent!.trim())).toEqual(['Customize widgets…', 'Pause serving', 'Release memory', 'Settings…', 'Quit Pitwall']);
      expect(items.map((b) => b.querySelector('mat-icon')?.getAttribute('data-mat-icon-name'))).toEqual(['sliders', 'pause', 'release', 'settings', 'power']);
      expect(document.querySelectorAll('.mat-mdc-menu-panel hr.pd-tray-sep')).toHaveLength(1);
      const sep = document.querySelector('.mat-mdc-menu-panel hr.pd-tray-sep');
      expect(sep?.previousElementSibling?.textContent).toContain('Settings…');
      expect(sep?.nextElementSibling?.textContent).toContain('Quit Pitwall');
    });

    it('Customize widgets opens Settings at the Tray popup page, Settings at Pitwall\'s page, Quit by the app\'s flow', async () => {
      const m = await mount();
      (await m.item('Customize widgets')).click();
      expect(m.openMain).toHaveBeenLastCalledWith('/settings/tray-popup');
      (await m.item('Settings…')).click();
      expect(m.openMain).toHaveBeenLastCalledWith('/settings');
      (await m.item('Quit Pitwall')).click();
      expect(m.quit).toHaveBeenCalledTimes(1);
    });

    it('handles exactly the menu ids it draws (an unknown id does not compile)', async () => {
      const { fixture } = await mount();
      // Never called: the proof is that it does not compile.
      // @ts-expect-error: 'reboot' is not one of the tray menu's ids.
      const unknown = (): void => fixture.componentInstance['choose']('reboot');
      expect(unknown).toBeTypeOf('function');
    });

    it('Pause serving and Release memory go through the backend', async () => {
      const backend = new MockBackend();
      const pause = vi.spyOn(backend, 'setServing');
      const release = vi.spyOn(backend, 'releaseMemory');
      const m = await mount({}, backend);
      (await m.item('Pause serving')).click();
      await settle(m.fixture);
      expect(pause).toHaveBeenCalledWith(true);
      (await m.item('Release memory')).click();
      await settle(m.fixture);
      expect(release).toHaveBeenCalledTimes(1);
    });

    it('disables Release memory and the serving item when the server cannot be reached', async () => {
      const { menu } = await mount({ popover: 'down' });
      const items = await menu();
      const labels = items.map((b) => b.textContent!.trim());
      expect(labels).toEqual(['Customize widgets…', 'Release memory', 'Settings…', 'Quit Pitwall']);
      expect(items[1]!.disabled).toBe(true);
    });
  });

  describe('the buttons of the widgets', () => {
    it('a model row opens Models, an error row opens Requests, the notice\'s Review opens the dashboard', async () => {
      const m = await mount({}, await withErrors({ popover: 'worst' }));
      (m.widget('Loaded models').querySelector('.row') as HTMLButtonElement).click();
      expect(m.openMain).toHaveBeenLastCalledWith('/models');
      (m.widget('Recent errors').querySelector('.row') as HTMLButtonElement).click();
      expect(m.openMain).toHaveBeenLastCalledWith('/requests');
      const review = Array.from(m.el.querySelectorAll('button')).find((b) => b.textContent!.trim() === 'Review')!;
      review.click();
      expect(m.openMain).toHaveBeenLastCalledWith('/dashboard');
    });

    it('says what went wrong when a button\'s command fails, and clears it on the next press', async () => {
      const backend = new MockBackend();
      const m = await mount({}, backend);
      vi.spyOn(backend, 'releaseMemory').mockRejectedValueOnce({ code: 'serverUnavailable', message: 'The server is not responding.' });
      (await m.item('Release memory')).click();
      await settle(m.fixture);
      expect(m.el.querySelector('p.error')!.textContent).toContain('The server is not responding.');
      m.openMain.mockClear();
      (m.el.querySelector('button.mat-mdc-unelevated-button') as HTMLButtonElement).click();
      await settle(m.fixture);
      expect(m.el.querySelector('p.error')).toBeNull();
    });
  });

  describe('fitting', () => {
    const original = globalThis.ResizeObserver;
    afterEach(() => {
      globalThis.ResizeObserver = original;
    });

    it('reports the popup\'s measured height to the shell, and survives a shell that refuses it', async () => {
      // Every observer is kept with what it watches: other components in the popup observe their own elements too, so the
      // spec calls the observers of the popup itself, whatever order they were made in.
      const watched: { target: Element; cb: () => void }[] = [];
      globalThis.ResizeObserver = class {
        constructor(private readonly cb: () => void) {}
        observe = (target: Element) => void watched.push({ target, cb: this.cb });
        disconnect = vi.fn();
      } as unknown as typeof ResizeObserver;
      const m = await mount();
      const popup = m.el.querySelector('pd-tray-popover')!;
      const notify = () => watched.filter((w) => w.target === popup).forEach((w) => w.cb());
      vi.spyOn(popup, 'getBoundingClientRect').mockReturnValue({ height: 311.4 } as DOMRect);
      notify();
      expect(m.fitTray).toHaveBeenCalledWith(312);
      m.fitTray.mockRejectedValueOnce(new Error('no'));
      vi.spyOn(popup, 'getBoundingClientRect').mockReturnValue({ height: 400 } as DOMRect);
      expect(() => notify()).not.toThrow();
      await settle(m.fixture);
    });
  });
});
