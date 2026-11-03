import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import type { MockBackend } from '../../core/mock-backend';
import { jobsProviders, mockWithJobs } from '../../../testing/job-test-support';
import { provideIcons } from '../../shared/icons';
import { HelperStore } from '../../state/helper.store';
import { ScheduleTab } from './schedule-tab';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

function render(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const { mock, jobs } = mockWithJobs({ now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([]), jobsProviders(jobs), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(ScheduleTab);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    await new Promise((r) => setTimeout(r, 2));
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
    f.detectChanges();
  }
};
const ROW = '[role="row"][data-key]';
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text))!;
const type = async (f: Parameters<typeof settle>[0], el: HTMLElement, label: string, value: string) => {
  const input = el.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!;
  input.value = value;
  input.dispatchEvent(new Event('input'));
  await settle(f);
};

describe('dy-schedule-tab', () => {
  beforeEach(() => vi.useFakeTimers({ now: NOW, toFake: ['Date'] }));
  afterEach(() => vi.useRealTimers());

  it('lists the schedules with their time, what follows, the last run and a status in words', async () => {
    const { f, el } = render({ wake: 'on' });
    await settle(f);
    const rows = [...el.querySelectorAll('[aria-label="Scheduled wakes"] ' + ROW)];
    expect(rows.map((r) => r.querySelector('strong')?.textContent)).toEqual(['Morning backup check', 'Wake the NAS for the weekend', 'Once, tomorrow']);
    expect(rows[0]!.textContent).toContain('Weekdays');
    expect(rows[0]!.textContent).toContain('Stay awake 30 min');
    expect(rows[0]!.textContent).toContain('Next wake');
    expect(rows[1]!.textContent).toContain('Missed: the computer was off or Dyno was not running then');
    expect(rows[2]!.textContent).toContain('Off');
    expect(el.textContent).toContain('A computer that is shut down is not woken.');
  });

  it('has a designed empty state', async () => {
    const { f, el } = render({ wake: 'empty' });
    await settle(f);
    expect(el.querySelector('.empty')?.textContent).toContain('No scheduled wakes yet');
  });

  it('shows every schedule paused, with one banner that turns scheduled wake on, when it is off', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelectorAll('.note')).toHaveLength(1);
    expect(el.querySelector('.note')?.textContent).toContain('Scheduled wake is turned off');
    expect(el.querySelector(ROW)?.textContent).toContain('Paused');
    expect(el.querySelector(ROW)?.textContent).not.toContain('Next wake');
    expect(btn(el, 'Add schedule').disabled).toBe(true);
    btn(el, 'Turn on scheduled wake').click();
    await settle(f);
    expect(el.querySelector(ROW)?.textContent).toContain('Next wake');
    expect(btn(el, 'Turn on scheduled wake')).toBeUndefined();
    expect(btn(el, 'Add schedule').disabled).toBe(false);
  });

  it('says "needs the helper" and turns it on from the tab', async () => {
    const { f, el } = render({ wake: 'needsHelper' });
    await settle(f);
    expect(el.querySelector('.note.warn')?.textContent).toContain('Setting a wake needs the administrator helper.');
    expect(el.querySelector(ROW)?.textContent).toContain('Needs the helper');
    expect(btn(el, 'Add schedule').getAttribute('matButton')).toBe('outlined');
    const helperStart = vi.spyOn(TestBed.inject(HelperStore), 'start').mockResolvedValue();
    btn(el, 'Turn on the helper').click();
    await settle(f);
    expect(helperStart).toHaveBeenCalledWith('turnOn');
  });

  it('says plainly when nothing can set a wake here, and disables Add schedule with the reason', async () => {
    const { f, el } = render({ wake: 'unavailable' });
    await settle(f);
    expect(el.querySelector('.note.warn')?.textContent).toContain('this build has none');
    expect(btn(el, 'Turn on the helper')).toBeUndefined();
    expect(btn(el, 'Add schedule').disabled).toBe(true);
    expect(btn(el, 'Add schedule').getAttribute('title')).toContain('this build has none');
    expect(el.textContent).not.toContain('Add schedule is unavailable');
    expect(btn(el, 'Retry')).toBeUndefined();
    expect(el.textContent?.match(/this build has none/g)).toHaveLength(1);
  });

  it('says what Windows cannot tell and where to check', async () => {
    const { f, el } = render({ wake: 'agent', os: 'windows' });
    await settle(f);
    expect(el.textContent).toContain('Windows wakes the computer for a timer that Dyno sets');
    const lines = [...el.querySelectorAll('ul.cant li')].map((l) => l.textContent);
    expect(lines.some((l) => l?.includes("Allow wake timers: Dyno can't tell. Check Power Options, Change advanced settings, Sleep."))).toBe(true);
    expect(lines.some((l) => l?.includes("Modern Standby: Dyno can't tell whether wake timers fire on this computer."))).toBe(true);
  });

  it('shows a row the OS refused as "Not set" with the reason and a Retry that tries again', async () => {
    const { f, el, mock } = render({ wake: 'failed' });
    await settle(f);
    expect(el.querySelector(ROW)?.textContent).toContain('Not set');
    expect(el.querySelector('.detail')?.textContent).toContain('The helper refused');
    const spy = vi.spyOn(mock, 'powerSetWake');
    btn(el, 'Retry').click();
    await settle(f);
    expect(spy).toHaveBeenCalledWith(expect.objectContaining({ id: 1, name: 'Morning backup check' }));
  });

  it('adds a schedule, with what happens after the wake', async () => {
    const { f, el } = render({ wake: 'on' });
    await settle(f);
    btn(el, 'Add schedule').click();
    await settle(f);
    await type(f, el, 'Name of the schedule', 'Evening check');
    await type(f, el, 'Time of day to wake', '6:30 pm');
    btn(el, 'Add schedule').click();
    await settle(f);
    expect(el.querySelector('section[aria-label="Scheduled wake"]')).toBeNull();
    expect([...el.querySelectorAll('[aria-label="Scheduled wakes"] ' + ROW)].map((r) => r.querySelector('strong')?.textContent)).toContain('Evening check');
  });

  it('chooses days with a Weekdays shortcut', async () => {
    const { f, el } = render({ wake: 'on' });
    await settle(f);
    btn(el, 'Add schedule').click();
    await settle(f);
    btn(el, 'On chosen days').click();
    await settle(f);
    expect(el.querySelectorAll('mat-button-toggle.mat-button-toggle-checked')).toHaveLength(1);
    const boxes = () => [...el.querySelectorAll<HTMLInputElement>('[aria-label="Days of the week"] mat-checkbox input')].map((c) => c.checked);
    expect(boxes()).toEqual([false, false, false, false, false, false, false]);
    btn(el, 'Weekdays').click();
    await settle(f);
    expect(boxes()).toEqual([true, true, true, true, true, false, false]);
  });

  it('shows the engine message under the field it is about, with the field in error and focused', async () => {
    const { f, el } = render({ wake: 'on' });
    await settle(f);
    btn(el, 'Add schedule').click();
    await settle(f);
    btn(el, 'Add schedule').click();
    await settle(f);
    expect(el.querySelector('mat-error')?.textContent).toContain('Give the schedule a name.');
    expect(el.querySelector('input[aria-label="Name of the schedule"]')?.getAttribute('aria-invalid')).toBe('true');
    expect(document.activeElement).toBe(el.querySelector('input[aria-label="Name of the schedule"]'));
  });

  it('shows a refusal about the choices under the "After it wakes" group, in error, with the group focused', async () => {
    const { f, el } = render({ wake: 'on' });
    await settle(f);
    btn(el, 'Add schedule').click();
    await settle(f);
    await type(f, el, 'Name of the schedule', 'Nothing');
    const stay = [...el.querySelectorAll<HTMLElement>('mat-checkbox')].find((c) => c.textContent?.includes('Stay awake for'))!;
    stay.querySelector<HTMLInputElement>('input')!.click();
    await settle(f);
    btn(el, 'Add schedule').click();
    await settle(f);
    expect(el.querySelector('fieldset.then .group-error')?.textContent).toContain('Choose what happens after the wake');
    expect(el.querySelector('fieldset.then')?.getAttribute('aria-invalid')).toBe('true');
    expect(el.querySelector('.note.bad')).toBeNull();
    expect(el.querySelector('fieldset.then')?.contains(document.activeElement)).toBe(true);
    expect(el.textContent).toContain('Wake these computers');
    expect(el.textContent).toContain('Run a Dyno job');
  });
});
