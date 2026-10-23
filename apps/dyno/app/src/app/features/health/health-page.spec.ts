import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { ViewPrefs } from '../../core/view-prefs';
import { HealthStore } from '../../state/health.store';
import { HealthPage, startupChoice } from './health-page';

function render(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
  const f = TestBed.createComponent(HealthPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 5; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};

describe('startupChoice', () => {
  it('maps a state to its segment', () => {
    expect(startupChoice('enabled')).toBe('enabled');
    expect(startupChoice('disabled')).toBe('disabled');
    expect(startupChoice({ delayed: 30 })).toBe('delayed');
  });
});

describe('dy-health-page', () => {
  it('shows the score and the startup items, and links to the tuning page', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Health');
    expect(el.querySelector('a[href="/tuning"]')?.textContent).toContain('Tuning suggestions');
    expect(el.querySelector('[aria-label="Tuning suggestions"]')).toBeNull();
    const store = TestBed.inject(HealthStore);
    expect(el.querySelectorAll('[aria-label="Startup manager"] tbody tr').length).toBe(store.startup().length);
  });

  it('lists the startup items with the highest impact first', async () => {
    const { f, el } = render();
    await settle(f);
    const order = { notMeasured: 0, low: 1, medium: 2, high: 3 } as const;
    const store = TestBed.inject(HealthStore);
    const names = [...el.querySelectorAll('[aria-label="Startup manager"] tbody tr td:first-child strong')].map((n) => n.textContent);
    const ranks = names.map((n) => order[store.startup().find((e) => e.name === n)!.impact.rating]);
    expect(ranks).toEqual([...ranks].sort((a, b) => b - a));
  });

  it('turns a startup item off and on with its switch and delays it with one click', async () => {
    const { f, el } = render();
    await settle(f);
    const store = TestBed.inject(HealthStore);
    const first = store.startup()[0]!;
    const sw = el.querySelector<HTMLElement>(`[aria-label="Start ${first.name} at sign-in"]`)!;
    expect(sw).not.toBeNull();
    sw.click();
    await settle(f);
    expect(startupChoice(store.startup()[0]!.state)).toBe(startupChoice(first.state) === 'disabled' ? 'enabled' : 'disabled');
    const delay = el.querySelector<HTMLButtonElement>(`button[aria-label="Delay ${store.startup()[1]!.name} at sign-in"]`);
    if (delay) {
      delay.click();
      await settle(f);
      expect(startupChoice(store.startup()[1]!.state)).toBe('delayed');
    }
  });

  it('an item that starts for every user asks for the helper first; saying no leaves it as it was', async () => {
    const { f, el, mock } = render();
    await settle(f);
    const store = TestBed.inject(HealthStore);
    const all = store.startup().find((e) => e.needsAdmin && e.state === 'enabled')!;
    el.querySelector<HTMLElement>(`[aria-label="Start ${all.name} at sign-in"]`)!.click();
    await settle(f);
    await new Promise((r) => setTimeout(r, 0));
    const dialog = document.querySelector('dy-helper-dialog') as HTMLElement;
    expect(dialog.textContent).toContain('Install the Dyno helper');
    [...dialog.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Not now')!.click();
    await settle(f);
    await new Promise((r) => setTimeout(r, 0));
    expect(store.startup().find((e) => e.id === all.id)!.state).toBe('enabled');
    expect((await mock.helperStatus()).state).toBe('notInstalled');
    // Once the helper is there the same switch just works.
    await mock.installHelper();
    el.querySelector<HTMLElement>(`[aria-label="Start ${all.name} at sign-in"]`)!.click();
    await settle(f);
    await new Promise((r) => setTimeout(r, 0));
    await settle(f);
    expect(store.startup().find((e) => e.id === all.id)!.state).toBe('disabled');
  });

  it('a failed startup list shows its own error with Retry', async () => {
    const { f, el } = render(['startupOverview']);
    await settle(f);
    const section = el.querySelector('[aria-label="Startup manager"]')!;
    expect(section.querySelector('[role="alert"]')).not.toBeNull();
  });

  describe('the startup filter', () => {
    beforeEach(() => localStorage.clear());
    afterEach(() => localStorage.clear());
    const toggles = (el: HTMLElement) => [...el.querySelectorAll<HTMLButtonElement>('[aria-label="Hide from the list"] button')];

    it('shows both counts, hides what the engine says is the OS or signed, and says how many it hid', async () => {
      const { f, el } = render();
      await settle(f);
      const store = TestBed.inject(HealthStore);
      const { total, os, signed } = store.startupCounts();
      expect(toggles(el).map((t) => t.getAttribute('aria-pressed'))).toEqual(['false', 'false']);
      expect(toggles(el)[0]!.textContent).toContain(String(os));
      expect(toggles(el)[1]!.textContent).toContain(String(signed));
      expect(el.querySelector('.shown')?.textContent).toContain(`Showing ${total} of ${total} entries.`);
      toggles(el)[0]!.click();
      await settle(f);
      expect(toggles(el)[0]!.getAttribute('aria-pressed')).toBe('true');
      expect(store.startup().length).toBe(total - os);
      expect(el.querySelector('.shown')?.textContent).toContain(`${os} hidden by the filter`);
      toggles(el)[1]!.click();
      await settle(f);
      expect(el.querySelectorAll('[aria-label="Startup manager"] tbody tr').length).toBe(total - os - signed);
    });

    it('remembers the choice for the next visit', async () => {
      const first = render();
      await settle(first.f);
      toggles(first.el)[1]!.click();
      await settle(first.f);
      TestBed.resetTestingModule();
      const second = render();
      await settle(second.f);
      expect(TestBed.inject(ViewPrefs).startupFilter()).toEqual({ hideOs: false, hideSigned: true });
      expect(toggles(second.el)[1]!.getAttribute('aria-pressed')).toBe('true');
    });

    it('with everything hidden it says so instead of claiming nothing starts', async () => {
      const { f, el } = render();
      await settle(f);
      toggles(el)[0]!.click();
      toggles(el)[1]!.click();
      await settle(f);
      const store = TestBed.inject(HealthStore);
      if (store.startup().length === 0) expect(el.textContent).toContain('Everything is hidden by the filter');
      else expect(el.querySelectorAll('[aria-label="Startup manager"] tbody tr').length).toBe(store.startup().length);
    });
  });
});
