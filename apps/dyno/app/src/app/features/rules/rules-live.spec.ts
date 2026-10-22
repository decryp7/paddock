import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { ActivityStore } from '../../state/activity.store';
import { LiveStore } from '../../state/live.store';
import { RulesStore } from '../../state/rules.store';
import { RulesLive } from './rules-live';

async function render(fail = new Set<string>()) {
  const mock = new MockBackend({ latencyMs: 0, now: () => Date.now(), fail });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const live = TestBed.inject(LiveStore);
  const f = TestBed.createComponent(RulesLive);
  f.detectChanges();
  const settle = async () => {
    for (let i = 0; i < 5; i++) {
      await f.whenStable();
      f.detectChanges();
    }
  };
  await new Promise((r) => setTimeout(r, 5));
  await settle();
  return { f, el: f.nativeElement as HTMLElement, mock, live, settle };
}

describe('dy-rules-live', () => {
  it('lists what is in effect and releases an app on a click', async () => {
    const { el, mock, live, settle } = await render();
    const actions = live.update()?.activeActions ?? [];
    expect(actions.length).toBeGreaterThan(0);
    const rows = el.querySelectorAll('[aria-label="In effect now"] tbody tr');
    expect(rows.length).toBe(actions.length);
    expect(rows[0]!.textContent).toContain(actions[0]!.description);
    const release = vi.spyOn(mock, 'releaseAction');
    el.querySelector<HTMLButtonElement>(`[aria-label="Release ${actions[0]!.app}"]`)!.click();
    await settle();
    expect(release).toHaveBeenCalledWith(actions[0]!.target);
    expect(TestBed.inject(RulesStore)).toBeTruthy();
  });

  it('shows a designed empty state when nothing is being throttled', async () => {
    const { el, live, settle, mock } = await render();
    live.receive({ ...mock.liveUpdate('summary'), activeActions: [] });
    await settle();
    expect(el.querySelector('[aria-label="In effect now"]')?.textContent).toContain('Nothing is being throttled');
  });

  it('lists at most ten rule and release entries, with Undo only where available', async () => {
    const { el, settle } = await render();
    const activity = TestBed.inject(ActivityStore);
    const rows = el.querySelectorAll('[aria-label="Recent activity"] tbody tr');
    expect(rows.length).toBeLessThanOrEqual(10);
    expect(rows.length).toBeGreaterThan(0);
    const kinds = new Set(activity.entries().filter((e) => e.kind === 'rule' || e.kind === 'release').map((e) => e.id));
    expect(rows.length).toBe(Math.min(10, kinds.size));
    const undo = el.querySelector<HTMLButtonElement>('[aria-label^="Undo: "]');
    expect(undo).not.toBeNull();
    const spy = vi.spyOn(activity, 'undo');
    undo!.click();
    await settle();
    expect(spy).toHaveBeenCalled();
  });

  it('shows an error with Retry when the activity fails', async () => {
    const fail = new Set(['activity']);
    const { el, settle } = await render(fail);
    expect(el.querySelector('[aria-label="Recent activity"] [role="alert"]')).not.toBeNull();
    fail.clear();
    el.querySelector<HTMLButtonElement>('[aria-label="Recent activity"] [role="alert"] button')!.click();
    await settle();
    expect(el.querySelector('[aria-label="Recent activity"] [role="alert"]')).toBeNull();
  });
});
