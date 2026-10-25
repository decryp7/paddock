import { TestBed } from '@angular/core/testing';
import type { DiskGuardPolicy, PausePolicy } from '../../core/api.types';
import { appProviders, byText, makeMock, text, type as typeInto } from '../../../testing/test-helpers';
import { DiskGuardEditor, diskGuardProblem, mbToGb } from './disk-guard-editor';
import { PauseEditor, pauseProblem } from './pause-editor';

const guard = (over: Partial<DiskGuardPolicy> = {}): DiskGuardPolicy => ({ enabled: true, drain_below_mb: 4096, resume_above_mb: 6144, reserve_mb: 1024, ...over });
const pause = (over: Partial<PausePolicy> = {}): PausePolicy => ({
  mode: 'off',
  windows: [{ days: ['mon'], start: '22:00', end: '06:00' }],
  busy_percent: 40,
  quiet_minutes: 5,
  ...over,
});

describe('diskGuardProblem', () => {
  it('requires resume above drain and reserve below drain', () => {
    expect(diskGuardProblem(guard())).toBeNull();
    expect(diskGuardProblem(guard({ resume_above_mb: 4096 }))).toContain('Resume above');
    expect(diskGuardProblem(guard({ reserve_mb: 4096 }))).toContain('reserve');
    expect(diskGuardProblem(guard({ drain_below_mb: 0 }))).toContain('more than 0');
    expect(diskGuardProblem(guard({ enabled: false, resume_above_mb: 1 }))).toBeNull();
    expect(mbToGb(1536)).toBe(1.5);
  });
});

describe('DiskGuardEditor', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: appProviders(makeMock()) }));
  function mount(policy = guard()) {
    const f = TestBed.createComponent(DiskGuardEditor);
    f.componentRef.setInput('policy', policy);
    f.detectChanges();
    const saved: DiskGuardPolicy[] = [];
    f.componentInstance.save.subscribe((p) => saved.push(p));
    return { f, el: f.nativeElement as HTMLElement, saved };
  }
  const save = (el: HTMLElement) => byText<HTMLButtonElement>(el, 'button', 'Save')!;

  it('shows GB, explains itself and saves edited MB', () => {
    const { f, el, saved } = mount();
    expect(text(el)).toContain('Pauses runners before the disk is full');
    expect(el.querySelector<HTMLInputElement>('#guard-drain_below_mb')!.value).toBe('4');
    expect(save(el).disabled).toBe(true);
    typeInto(el.querySelector('#guard-resume_above_mb')!, '8', 'change');
    f.detectChanges();
    save(el).click();
    expect(saved).toEqual([guard({ resume_above_mb: 8192 })]);
  });

  it('blocks saving an inconsistent policy and shows why', () => {
    const { f, el } = mount();
    typeInto(el.querySelector('#guard-resume_above_mb')!, '3', 'change');
    f.detectChanges();
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('Resume above must be larger');
    expect(save(el).disabled).toBe(true);
    typeInto(el.querySelector('#guard-resume_above_mb')!, '6', 'change');
    typeInto(el.querySelector('#guard-reserve_mb')!, '5', 'change');
    f.detectChanges();
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('reserve');
    byText<HTMLButtonElement>(el, 'button', 'Discard changes')!.click();
    f.detectChanges();
    expect(el.querySelector('[role="alert"]')).toBeNull();
  });

  it('ignores junk input, toggles off and honours read-only', () => {
    const { f, el, saved } = mount();
    typeInto(el.querySelector('#guard-reserve_mb')!, 'abc', 'change');
    typeInto(el.querySelector('#guard-reserve_mb')!, '-1', 'change');
    f.detectChanges();
    expect(save(el).disabled).toBe(true);
    (el.querySelector('mat-slide-toggle [role="switch"]') as HTMLElement).click();
    f.detectChanges();
    expect(el.querySelector<HTMLInputElement>('#guard-reserve_mb')!.disabled).toBe(true);
    save(el).click();
    expect(saved[0]!.enabled).toBe(false);
    f.componentRef.setInput('readOnly', true);
    f.detectChanges();
    expect(byText(el, 'button', 'Save')).toBeNull();
  });
});

describe('pauseProblem', () => {
  it('checks the active mode only', () => {
    expect(pauseProblem(pause())).toBeNull();
    expect(pauseProblem(pause({ mode: 'schedule', windows: [] }))).toContain('at least one window');
    expect(pauseProblem(pause({ mode: 'schedule', windows: [{ days: [], start: '01:00', end: '02:00' }] }))).toContain('one day');
    expect(pauseProblem(pause({ mode: 'schedule', windows: Array(15).fill({ days: ['mon'], start: '01:00', end: '02:00' }) }))).toContain('14');
    expect(pauseProblem(pause({ mode: 'when-idle', busy_percent: 4 }))).toContain('5 and 100');
    expect(pauseProblem(pause({ mode: 'when-idle', quiet_minutes: 241 }))).toContain('1 and 240');
    expect(pauseProblem(pause({ mode: 'when-idle' }))).toBeNull();
  });
});

describe('PauseEditor', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: appProviders(makeMock()) }));
  function mount(policy = pause()) {
    const f = TestBed.createComponent(PauseEditor);
    f.componentRef.setInput('policy', policy);
    f.detectChanges();
    const saved: PausePolicy[] = [];
    f.componentInstance.save.subscribe((p) => saved.push(p));
    return { f, el: f.nativeElement as HTMLElement, saved };
  }
  const save = (el: HTMLElement) => byText<HTMLButtonElement>(el, 'button', 'Save')!;
  const radio = (el: HTMLElement, v: string) => el.querySelector<HTMLInputElement>(`input[type="radio"][value="${v}"]`)!;

  it('switches between the three modes and shows the matching fields', () => {
    const { f, el } = mount();
    expect(radio(el, 'off').checked).toBe(true);
    expect(el.querySelector('.window')).toBeNull();
    radio(el, 'schedule').click();
    f.detectChanges();
    expect(el.querySelectorAll('.window')).toHaveLength(1);
    radio(el, 'when-idle').click();
    f.detectChanges();
    expect(el.querySelector('.window')).toBeNull();
    expect(el.querySelector<HTMLInputElement>('#pause-busy')!.value).toBe('40');
    expect(el.querySelector<HTMLInputElement>('#pause-quiet')!.value).toBe('5');
  });

  it('edits, adds and removes windows (at most 14)', () => {
    const { f, el, saved } = mount(pause({ mode: 'schedule' }));
    el.querySelector<HTMLInputElement>('.day input')!.click(); // mon off
    f.detectChanges();
    expect(save(el).disabled).toBe(true);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('at least one day');
    el.querySelectorAll<HTMLInputElement>('.day input')[5]!.click(); // sat on
    f.detectChanges();
    expect(save(el).disabled).toBe(false);
    const times = el.querySelectorAll<HTMLInputElement>('input[type="time"]');
    typeInto(times[0]!, '23:30', 'change');
    typeInto(times[1]!, '', 'change');
    f.detectChanges();
    expect(times[0]!.value).toBe('23:30');
    byText<HTMLButtonElement>(el, 'button', 'Add window')!.click();
    f.detectChanges();
    expect(el.querySelectorAll('.window')).toHaveLength(2);
    byText<HTMLButtonElement>(el, 'button', 'Remove')!.click();
    f.detectChanges();
    expect(el.querySelectorAll('.window')).toHaveLength(1);
    save(el).click();
    expect(saved[0]!.windows).toEqual([{ days: ['mon', 'tue', 'wed', 'thu', 'fri'], start: '22:00', end: '06:00' }]);
    for (let i = 0; i < 20; i++) byText<HTMLButtonElement>(el, 'button', 'Add window')!.click();
    f.detectChanges();
    expect(el.querySelectorAll('.window')).toHaveLength(14);
    expect(byText<HTMLButtonElement>(el, 'button', 'Add window')!.disabled).toBe(true);
    expect(text(el)).toContain('At most 14 windows');
  });

  it('validates the idle thresholds', () => {
    const { f, el, saved } = mount(pause({ mode: 'when-idle' }));
    typeInto(el.querySelector('#pause-busy')!, '3', 'change');
    f.detectChanges();
    expect(save(el).disabled).toBe(true);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('5 and 100');
    typeInto(el.querySelector('#pause-busy')!, '80', 'change');
    typeInto(el.querySelector('#pause-quiet')!, '30', 'change');
    f.detectChanges();
    save(el).click();
    expect(saved[0]).toMatchObject({ mode: 'when-idle', busy_percent: 80, quiet_minutes: 30 });
  });
});
