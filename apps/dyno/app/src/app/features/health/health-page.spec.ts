import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import type { ReadVia } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { ViewPrefs } from '../../core/view-prefs';
import { HealthStore } from '../../state/health.store';
import { HealthPage } from './health-page';

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
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
    f.detectChanges();
  }
};

describe('dy-health-page', () => {
  it('shows the score and the start-up items', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Health');
    expect(el.querySelector('a[href="/tuning"]')).toBeNull();
    const store = TestBed.inject(HealthStore);
    expect(el.querySelectorAll('[aria-label="Start-up items"] [role="row"][data-key]').length).toBe(store.startup().length);
  });

  it('only shows start-up items: no switch, no delay, no helper prompt', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelectorAll('dy-switch, [role="switch"]')).toHaveLength(0);
    expect(el.textContent).not.toContain('Delay');
  });

  it('says who read the login items: the helper chip, or the reason once with the button to turn the helper on', async () => {
    const { f, el, mock } = render();
    await settle(f);
    const real = mock.startupOverview.bind(mock);
    vi.spyOn(mock, 'startupOverview').mockImplementation(async (filter) => ({ ...(await real(filter)), btmVia: 'helper' }));
    await TestBed.inject(HealthStore).loadStartup(TestBed.inject(ViewPrefs).startupFilter());
    await settle(f);
    expect(el.querySelector('[data-testid="reading-source-helper"]')?.textContent).toContain('administrator helper');
    const reason = 'Login items (System Settings > Login Items & Extensions) need administrator rights to read.';
    const via: ReadVia = { needsHelper: { reason, next: { openSettings: 'helper' } } };
    vi.spyOn(mock, 'startupOverview').mockImplementation(async (filter) => ({ ...(await real(filter)), btmVia: via, unavailable: [reason, 'The user launch agents folder could not be read.'] }));
    await TestBed.inject(HealthStore).loadStartup(TestBed.inject(ViewPrefs).startupFilter());
    await settle(f);
    expect(el.querySelector('[data-testid="reading-source-needs"]')?.textContent).toContain('administrator rights');
    expect(el.querySelector('[data-testid="reading-source-turn-on"]')?.textContent).toContain('Turn on administrator readings');
    const notes = [...el.querySelectorAll('.shown[role="status"]')].map((n) => n.textContent);
    expect(notes).toEqual(['Not shown: The user launch agents folder could not be read.']);
  });

  it('lists the startup items with the highest impact first', async () => {
    const { f, el } = render();
    await settle(f);
    const order = { notMeasured: 0, low: 1, medium: 2, high: 3 } as const;
    const store = TestBed.inject(HealthStore);
    const names = [...el.querySelectorAll('[aria-label="Start-up items"] [role="row"][data-key] strong')].map((n) => n.textContent);
    const ranks = names.map((n) => order[store.startup().find((e) => e.name === n)!.impact.rating]);
    expect(ranks).toEqual([...ranks].sort((a, b) => b - a));
  });

  it('a failed startup list shows its own error with Retry', async () => {
    const { f, el } = render(['startupOverview']);
    await settle(f);
    const section = el.querySelector('[aria-label="Start-up items"]')!;
    expect(section.querySelector('[role="alert"]')).not.toBeNull();
  });

  describe('the startup filter', () => {
    beforeEach(() => localStorage.clear());
    afterEach(() => localStorage.clear());
    const toggles = (el: HTMLElement) => [...el.querySelectorAll<HTMLInputElement>('[aria-label="Hide from the list"] input[type="checkbox"]')];

    it('shows both counts, hides what the engine says is the OS or signed, and says how many it hid', async () => {
      const { f, el } = render();
      await settle(f);
      const store = TestBed.inject(HealthStore);
      const { total, os, signed } = store.startupCounts();
      expect(toggles(el).map((t) => String(t.checked))).toEqual(['false', 'false']);
      expect(toggles(el)[0]!.closest('mat-checkbox')!.textContent).toContain(String(os));
      expect(toggles(el)[1]!.closest('mat-checkbox')!.textContent).toContain(String(signed));
      expect(el.querySelector('.shown')?.textContent).toContain(`Showing ${total} of ${total} entries.`);
      toggles(el)[0]!.click();
      await settle(f);
      expect(toggles(el)[0]!.checked).toBe(true);
      expect(store.startup().length).toBe(total - os);
      expect(el.querySelector('.shown')?.textContent).toContain(`${os} hidden by the filter`);
      toggles(el)[1]!.click();
      await settle(f);
      expect(el.querySelectorAll('[aria-label="Start-up items"] [role="row"][data-key]').length).toBe(total - os - signed);
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
      expect(toggles(second.el)[1]!.checked).toBe(true);
    });

    it('with everything hidden it says so instead of claiming nothing starts', async () => {
      const { f, el } = render();
      await settle(f);
      toggles(el)[0]!.click();
      toggles(el)[1]!.click();
      await settle(f);
      const store = TestBed.inject(HealthStore);
      if (store.startup().length === 0) expect(el.textContent).toContain('Everything is hidden by the filter');
      else expect(el.querySelectorAll('[aria-label="Start-up items"] [role="row"][data-key]').length).toBe(store.startup().length);
    });
  });
});
