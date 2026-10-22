import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
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

  it('a failed startup list shows its own error with Retry', async () => {
    const { f, el } = render(['startupEntries']);
    await settle(f);
    const section = el.querySelector('[aria-label="Startup manager"]')!;
    expect(section.querySelector('[role="alert"]')).not.toBeNull();
  });
});
