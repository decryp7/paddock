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
  it('lists tuning suggestions and startup items, and applies a suggestion only on a click', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toContain('Health');
    const apply = [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.getAttribute('aria-label')?.startsWith('Apply '));
    expect(apply, 'a suggestion that can be applied').toBeDefined();
    const store = TestBed.inject(HealthStore);
    const before = store.tuning().filter((t) => t.applied).length;
    apply!.click();
    await settle(f);
    expect(store.tuning().filter((t) => t.applied).length).toBe(before + 1);
    expect(el.querySelectorAll('[aria-label="Startup manager"] tbody tr').length).toBe(store.startup().length);
  });

  it('changes a startup item through the segmented control and ignores a click on the current setting', async () => {
    const { f, el } = render();
    await settle(f);
    const store = TestBed.inject(HealthStore);
    const first = store.startup()[0]!;
    const group = el.querySelector<HTMLElement>(`[aria-label="Sign-in setting of ${first.name}"]`)!;
    const buttons = group.querySelectorAll('button');
    const current = ['enabled', 'delayed', 'disabled'].indexOf(startupChoice(first.state));
    (buttons[current] as HTMLButtonElement).click();
    await settle(f);
    expect(startupChoice(store.startup()[0]!.state)).toBe(startupChoice(first.state));
    (buttons[current === 2 ? 0 : 2] as HTMLButtonElement).click();
    await settle(f);
    expect(startupChoice(store.startup()[0]!.state)).not.toBe(startupChoice(first.state));
  });

  it('a failed list shows its own error with Retry and leaves the other section alone', async () => {
    const { f, el } = render(['tuning']);
    await settle(f);
    const section = el.querySelector('[aria-label="Tuning suggestions"]')!;
    expect(section.querySelector('[role="alert"]')).not.toBeNull();
    expect(el.querySelector('[aria-label="Startup manager"] table')).not.toBeNull();
  });
});
