import { TestBed } from '@angular/core/testing';
import type { CleanupPolicy, UpdatePolicy } from '../../core/api.types';
import { appProviders, byText, makeMock, text, type as typeInto } from '../../../testing/test-helpers';
import { CleanupPolicyEditor } from './cleanup-policy-editor';
import { UpdatePolicyEditor, positiveInt } from './update-policy-editor';

describe('positiveInt', () => {
  it('accepts positive whole numbers only', () => {
    expect(positiveInt('5')).toBe(5);
    expect(positiveInt(' 12 ')).toBe(12);
    for (const bad of ['0', '-3', '1.5', 'abc', '']) expect(positiveInt(bad)).toBeNull();
  });
});

const update = (over: Partial<UpdatePolicy> = {}): UpdatePolicy => ({
  mode: 'automatic',
  pinned: null,
  window: null,
  check_every_hours: 6,
  channel: 'stable',
  skipped: [],
  deadline_days: 30,
  ...over,
});

describe('UpdatePolicyEditor', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: appProviders(makeMock()) }));

  function mount(policy: UpdatePolicy, inputs: Record<string, unknown> = {}) {
    const f = TestBed.createComponent(UpdatePolicyEditor);
    f.componentRef.setInput('policy', policy);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    const saved: UpdatePolicy[] = [];
    f.componentInstance.save.subscribe((p) => saved.push(p));
    return { f, el: f.nativeElement as HTMLElement, saved };
  }
  const save = (el: HTMLElement) => byText<HTMLButtonElement>(el, 'button', /Save|Saving/)!;

  it('shows the current policy and has nothing to save yet', () => {
    const { el } = mount(update());
    expect((el.querySelector('input[type="radio"][value="automatic"]') as HTMLInputElement).checked).toBe(true);
    expect((el.querySelector('input[type="number"]') as HTMLInputElement).value).toBe('6');
    expect(save(el).disabled).toBe(true);
    expect(byText(el, 'button', 'Discard changes')).toBeNull();
    expect(text(el)).toContain('Ask first');
  });

  it('edits mode, channel and hours, then emits the draft', () => {
    const { f, el, saved } = mount(update());
    (el.querySelector('input[type="radio"][value="notify-only"]') as HTMLInputElement).click();
    byText<HTMLButtonElement>(el, 'button', 'Pre-release')!.click();
    const hours = el.querySelector('input[type="number"]') as HTMLInputElement;
    typeInto(hours, '0', 'change');
    typeInto(hours, '12', 'change');
    f.detectChanges();
    expect(save(el).disabled).toBe(false);
    save(el).click();
    expect(saved).toEqual([update({ mode: 'notify-only', channel: 'prerelease', check_every_hours: 12 })]);
  });

  it('disables the schedule for manual mode', () => {
    const { el } = mount(update({ mode: 'manual' }));
    expect((el.querySelector('input[type="number"]') as HTMLInputElement).disabled).toBe(true);
  });

  it('manages the maintenance window', () => {
    const { f, el, saved } = mount(update());
    const toggle = el.querySelector('.check input') as HTMLInputElement;
    toggle.click();
    f.detectChanges();
    const days = Array.from(el.querySelectorAll<HTMLButtonElement>('.day'));
    expect(days.map((d) => d.getAttribute('aria-pressed'))).toEqual(['true', 'true', 'true', 'true', 'true', 'false', 'false']);
    days[5]!.click();
    days[0]!.click();
    f.detectChanges();
    const times = el.querySelectorAll<HTMLInputElement>('input[type="time"]');
    typeInto(times[0]!, '23:00', 'change');
    typeInto(times[1]!, 'bad', 'change');
    typeInto(times[1]!, '05:00', 'change');
    f.detectChanges();
    expect(text(el)).toContain('spans midnight');
    save(el).click();
    expect(saved[0]!.window).toEqual({ days: ['tue', 'wed', 'thu', 'fri', 'sat'], start: '23:00', end: '05:00' });
    toggle.click();
    f.detectChanges();
    expect(el.querySelector('.window')).toBeNull();
  });

  it('discards changes', () => {
    const { f, el } = mount(update());
    byText<HTMLButtonElement>(el, 'button', 'Stable')!.click();
    byText<HTMLButtonElement>(el, 'button', 'Pre-release')!.click();
    f.detectChanges();
    byText<HTMLButtonElement>(el, 'button', 'Discard changes')!.click();
    f.detectChanges();
    expect(byText(el, 'button', 'Pre-release')!.getAttribute('aria-checked')).toBe('false');
    expect(save(el).disabled).toBe(true);
  });

  it('is read-only on request and shows saving', () => {
    const { el } = mount(update(), { readOnly: true });
    expect(byText(el, 'button', 'Save')).toBeNull();
    expect(el.querySelector('fieldset')?.hasAttribute('disabled')).toBe(true);
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: appProviders(makeMock()) });
    const s = mount(update(), { saving: true });
    expect(save(s.el).textContent).toContain('Saving…');
  });

  it('resets the draft when a new policy arrives', () => {
    const { f, el } = mount(update());
    f.componentRef.setInput('policy', update({ check_every_hours: 24 }));
    f.detectChanges();
    expect((el.querySelector('input[type="number"]') as HTMLInputElement).value).toBe('24');
  });
});

const cleanup = (over: Partial<CleanupPolicy> = {}): CleanupPolicy => ({
  automatic: true,
  every_hours: 24,
  work_max_age_hours: 72,
  cache_cap_mb: 10000,
  folder_cap_mb: 50000,
  min_free_mb: 20000,
  keep_previous: true,
  ...over,
});

describe('CleanupPolicyEditor', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: appProviders(makeMock()) }));

  function mount(policy: CleanupPolicy, readOnly = false) {
    const f = TestBed.createComponent(CleanupPolicyEditor);
    f.componentRef.setInput('policy', policy);
    f.componentRef.setInput('readOnly', readOnly);
    f.detectChanges();
    const saved: CleanupPolicy[] = [];
    f.componentInstance.save.subscribe((p) => saved.push(p));
    return { f, el: f.nativeElement as HTMLElement, saved };
  }

  it('shows hours as they are and MB fields in GB', () => {
    const { el } = mount(cleanup());
    const values = Array.from(el.querySelectorAll<HTMLInputElement>('input[type="number"]')).map((i) => i.value);
    expect(values).toEqual(['24', '72', '10', '50', '20']);
  });

  it('converts GB back to MB, ignores invalid input and emits', () => {
    const { f, el, saved } = mount(cleanup());
    const inputs = el.querySelectorAll<HTMLInputElement>('input[type="number"]');
    typeInto(inputs[0]!, '12', 'change');
    typeInto(inputs[1]!, '1.5', 'change');
    typeInto(inputs[2]!, '2.5', 'change');
    typeInto(inputs[3]!, '-1', 'change');
    typeInto(inputs[4]!, '0', 'change');
    (el.querySelector('input[type="checkbox"]') as HTMLInputElement).click();
    f.detectChanges();
    byText<HTMLButtonElement>(el, 'button', 'Save')!.click();
    expect(saved).toEqual([cleanup({ every_hours: 12, cache_cap_mb: 2500, keep_previous: false })]);
  });

  it('toggles automatic cleanup and discards changes', () => {
    const { f, el } = mount(cleanup());
    const toggle = el.querySelector('mat-slide-toggle button') as HTMLButtonElement;
    toggle.click();
    f.detectChanges();
    expect(byText(el, 'button', 'Discard changes')).not.toBeNull();
    byText<HTMLButtonElement>(el, 'button', 'Discard changes')!.click();
    f.detectChanges();
    expect((el.querySelector('mat-slide-toggle button') as HTMLButtonElement).getAttribute('aria-checked')).toBe('true');
    expect(byText<HTMLButtonElement>(el, 'button', 'Save')!.disabled).toBe(true);
  });

  it('has no Save when read-only', () => {
    const { el } = mount(cleanup(), true);
    expect(byText(el, 'button', 'Save')).toBeNull();
    expect((el.querySelector('input[type="number"]') as HTMLInputElement).disabled).toBe(true);
  });
});

describe('deadline field', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: appProviders(makeMock()) }));

  it('edits deadline_days and explains that forges enforce none', () => {
    const f = TestBed.createComponent(UpdatePolicyEditor);
    f.componentRef.setInput('policy', update());
    f.detectChanges();
    const saved: UpdatePolicy[] = [];
    f.componentInstance.save.subscribe((p) => saved.push(p));
    const el = f.nativeElement as HTMLElement;
    const field = el.querySelector<HTMLInputElement>('#upd-deadline')!;
    expect(field.value).toBe('30');
    expect(text(el)).toContain('0 = never');
    expect(text(el)).toContain('Gitea and Forgejo enforce no deadline');
    typeInto(field, '-2', 'change');
    typeInto(field, '1.5', 'change');
    f.detectChanges();
    expect(byText<HTMLButtonElement>(el, 'button', 'Save')!.disabled).toBe(true);
    typeInto(field, '0', 'change');
    f.detectChanges();
    byText<HTMLButtonElement>(el, 'button', 'Save')!.click();
    expect(saved[0]!.deadline_days).toBe(0);
  });
});
