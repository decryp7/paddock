import { TestBed } from '@angular/core/testing';
import type { ProcessOverride, ProcessPolicy } from '../../core/api.types';
import { DEFAULT_PROCESS_POLICY } from '../../core/mock/mock-processes';
import { appProviders, byText, makeMock, text, type as typeInto } from '../../../testing/test-helpers';
import { KeepInput, keepAmount } from './keep-input';
import { ProcessOverrideEditor, normalizeOverride } from './process-override-editor';
import { ProcessPolicyEditor } from './process-policy-editor';
import { NEW_RULE, ProcessRulesEditor, moveRule } from './process-rules-editor';

describe('pure helpers', () => {
  it('reads keep amounts', () => {
    expect(keepAmount('3')).toBe(3);
    for (const bad of ['0', '-1', '1.5', '', 'x']) expect(keepAmount(bad)).toBeNull();
  });
  it('moves rules within bounds', () => {
    expect(moveRule(['a', 'b', 'c'], 1, -1)).toEqual(['b', 'a', 'c']);
    expect(moveRule(['a', 'b', 'c'], 1, 1)).toEqual(['a', 'c', 'b']);
    expect(moveRule(['a', 'b'], 0, -1)).toEqual(['a', 'b']);
    expect(moveRule(['a', 'b'], 1, 1)).toEqual(['a', 'b']);
  });
  it('drops empty overrides', () => {
    expect(normalizeOverride({})).toBeNull();
    expect(normalizeOverride({ rules: [] })).toBeNull();
    expect(normalizeOverride({ cleanup: false, rules: [] })).toEqual({ cleanup: false });
    expect(normalizeOverride({ keep: { amount: 1, unit: 'hours' }, rules: [NEW_RULE] })).toEqual({ keep: { amount: 1, unit: 'hours' }, rules: [NEW_RULE] });
  });
});

describe('process policy editors', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: appProviders(makeMock()) }));

  it('KeepInput emits a new amount or unit and restores an invalid amount', () => {
    const f = TestBed.createComponent(KeepInput);
    f.componentRef.setInput('value', { amount: 10, unit: 'minutes' });
    f.componentRef.setInput('label', 'Keep for');
    f.detectChanges();
    const out: unknown[] = [];
    f.componentInstance.changed.subscribe((v) => out.push(v));
    const el = f.nativeElement as HTMLElement;
    const amount = el.querySelector('input') as HTMLInputElement;
    expect(amount.getAttribute('aria-label')).toBe('Keep for amount');
    typeInto(amount, '30', 'change');
    typeInto(amount, '0', 'change');
    expect(amount.value).toBe('10');
    typeInto(el.querySelector('select') as HTMLSelectElement, 'hours', 'change');
    expect(out).toEqual([
      { amount: 30, unit: 'minutes' },
      { amount: 10, unit: 'hours' },
    ]);
  });

  describe('rules editor', () => {
    function mount(rules = DEFAULT_PROCESS_POLICY.rules, readOnly = false) {
      const f = TestBed.createComponent(ProcessRulesEditor);
      f.componentRef.setInput('rules', rules);
      f.componentRef.setInput('readOnly', readOnly);
      f.detectChanges();
      const out: unknown[][] = [];
      f.componentInstance.rulesChange.subscribe((r) => out.push(r));
      return { el: f.nativeElement as HTMLElement, out };
    }
    const btn = (el: HTMLElement, label: string) => el.querySelector(`button[aria-label="${label}"]`) as HTMLButtonElement;

    it('lists the rules in order, with a note that the first match wins', () => {
      const { el } = mount();
      expect(el.querySelectorAll('tbody tr')).toHaveLength(5);
      expect((el.querySelector('input[aria-label="Pattern of rule 1"]') as HTMLInputElement).value).toBe('*GradleDaemon*');
      expect(text(el)).toContain('first match wins');
      expect(btn(el, 'Move rule 1 up').disabled).toBe(true);
      expect(btn(el, 'Move rule 5 down').disabled).toBe(true);
      expect((el.querySelector('select[aria-label="What rule 5 does"]') as HTMLSelectElement).value).toBe('never');
    });

    it('edits patterns, modes and keep times, moves, removes and adds', () => {
      const { el, out } = mount([{ pattern: 'a', keep: { amount: 3, unit: 'hours' } }, { pattern: 'b', keep: 'never' }]);
      typeInto(el.querySelector('input[aria-label="Pattern of rule 1"]') as HTMLInputElement, 'aa');
      expect(out.at(-1)![0]).toMatchObject({ pattern: 'aa' });
      typeInto(el.querySelector('select[aria-label="What rule 1 does"]') as HTMLSelectElement, 'never', 'change');
      expect(out.at(-1)![0]).toMatchObject({ keep: 'never' });
      typeInto(el.querySelector('select[aria-label="What rule 2 does"]') as HTMLSelectElement, 'for', 'change');
      expect(out.at(-1)![1]).toMatchObject({ keep: { amount: 3, unit: 'hours' } });
      typeInto(el.querySelector('input[aria-label="Keep time of rule 1 amount"]') as HTMLInputElement, '5', 'change');
      expect(out.at(-1)![0]).toMatchObject({ keep: { amount: 5, unit: 'hours' } });
      btn(el, 'Move rule 1 down').click();
      expect((out.at(-1)![0] as { pattern: string }).pattern).toBe('b');
      btn(el, 'Remove rule 1').click();
      expect(out.at(-1)).toHaveLength(1);
      byText<HTMLButtonElement>(el, 'button', 'Add rule')!.click();
      expect(out.at(-1)).toHaveLength(3);
    });

    it('says so with no rules and locks everything when read-only', () => {
      const { el } = mount([], false);
      expect(text(el)).toContain('No rules');
      const ro = mount(DEFAULT_PROCESS_POLICY.rules, true).el;
      expect(btn(ro, 'Remove rule 1').disabled).toBe(true);
      expect(byText<HTMLButtonElement>(ro, 'button', 'Add rule')!.disabled).toBe(true);
    });
  });

  describe('defaults editor', () => {
    function mount(policy: ProcessPolicy = DEFAULT_PROCESS_POLICY, readOnly = false) {
      const f = TestBed.createComponent(ProcessPolicyEditor);
      f.componentRef.setInput('policy', policy);
      f.componentRef.setInput('readOnly', readOnly);
      f.detectChanges();
      const saved: ProcessPolicy[] = [];
      f.componentInstance.save.subscribe((p) => saved.push(p));
      return { f, el: f.nativeElement as HTMLElement, saved };
    }
    const saveBtn = (el: HTMLElement) => byText<HTMLButtonElement>(el, 'button', /^(Save|Saving)/)!;

    it('shows the policy and has nothing to save', () => {
      const { el } = mount();
      expect(text(el)).toContain('Stop leftover processes automatically');
      expect((el.querySelector('input[aria-label="Keep for amount"]') as HTMLInputElement).value).toBe('10');
      expect((el.querySelector('input[type="number"][id="processes-timeout"]') as HTMLInputElement).value).toBe('15');
      expect(saveBtn(el).disabled).toBe(true);
      expect(byText(el, 'button', 'Discard changes')).toBeNull();
    });

    it('edits the keep time, timeout and rules, saves, and discards', async () => {
      const { f, el, saved } = mount();
      typeInto(el.querySelector('input[aria-label="Keep for amount"]') as HTMLInputElement, '20', 'change');
      typeInto(el.querySelector('#processes-timeout') as HTMLInputElement, '30', 'change');
      typeInto(el.querySelector('#processes-timeout') as HTMLInputElement, '0', 'change');
      expect((el.querySelector('#processes-timeout') as HTMLInputElement).value).toBe('30');
      byText<HTMLButtonElement>(el, 'button', 'Add rule')!.click();
      el.querySelector<HTMLElement>('mat-slide-toggle button')!.click();
      f.detectChanges();
      expect(saveBtn(el).disabled).toBe(false);
      saveBtn(el).click();
      expect(saved[0]).toMatchObject({ cleanup: false, keep: { amount: 20, unit: 'minutes' }, stop_timeout_secs: 30 });
      expect(saved[0]!.rules).toHaveLength(6);
      byText<HTMLButtonElement>(el, 'button', 'Discard changes')!.click();
      f.detectChanges();
      expect(saveBtn(el).disabled).toBe(true);
    });

    it('is read-only on request and shows Saving…', () => {
      const { f, el } = mount(DEFAULT_PROCESS_POLICY, true);
      expect(byText(el, 'button', /^Save/)).toBeNull();
      f.componentRef.setInput('readOnly', false);
      f.componentRef.setInput('saving', true);
      f.detectChanges();
      expect(saveBtn(el).textContent).toContain('Saving…');
    });
  });

  describe('override editor', () => {
    function mount(override: ProcessOverride | null = null) {
      const f = TestBed.createComponent(ProcessOverrideEditor);
      f.componentRef.setInput('override', override);
      f.componentRef.setInput('defaults', DEFAULT_PROCESS_POLICY);
      f.detectChanges();
      const saved: (ProcessOverride | null)[] = [];
      f.componentInstance.save.subscribe((p) => saved.push(p));
      return { f, el: f.nativeElement as HTMLElement, saved };
    }
    const saveBtn = (el: HTMLElement) => byText<HTMLButtonElement>(el, 'button', /^(Save|Saving)/)!;

    it('follows the defaults until told otherwise', () => {
      const { el } = mount();
      const sel = el.querySelector('select') as HTMLSelectElement;
      expect(sel.value).toBe('inherit');
      expect(text(el)).toContain('Follow the defaults (on)');
      expect(text(el)).toContain('default 10 min');
      expect(saveBtn(el).disabled).toBe(true);
    });

    it('sets cleanup, its own keep time and rules, saves them, then clears', () => {
      const { f, el, saved } = mount();
      typeInto(el.querySelector('select') as HTMLSelectElement, 'off', 'change');
      const own = el.querySelector('input[type="checkbox"]') as HTMLInputElement;
      own.click();
      f.detectChanges();
      typeInto(el.querySelector('input[aria-label="Keep for amount"]') as HTMLInputElement, '45', 'change');
      byText<HTMLButtonElement>(el, 'button', 'Add rule')!.click();
      f.detectChanges();
      saveBtn(el).click();
      expect(saved[0]).toMatchObject({ cleanup: false, keep: { amount: 45, unit: 'minutes' } });
      expect(saved[0]!.rules).toHaveLength(1);
      own.click();
      typeInto(el.querySelector('select') as HTMLSelectElement, 'on', 'change');
      expect(el.querySelector('select')!.value).toBe('on');
      typeInto(el.querySelector('select') as HTMLSelectElement, 'inherit', 'change');
      byText<HTMLButtonElement>(el, 'button', 'Discard changes')!.click();
      f.detectChanges();
      expect(saveBtn(el).disabled).toBe(true);
    });

    it('saves null when every override is removed', () => {
      const { f, el, saved } = mount({ cleanup: false });
      typeInto(el.querySelector('select') as HTMLSelectElement, 'inherit', 'change');
      f.detectChanges();
      saveBtn(el).click();
      expect(saved).toEqual([null]);
    });

    it('is read-only on request', () => {
      const { f, el } = mount({ cleanup: true });
      f.componentRef.setInput('readOnly', true);
      f.detectChanges();
      expect(byText(el, 'button', /^Save/)).toBeNull();
      expect((el.querySelector('select') as HTMLSelectElement).disabled).toBe(true);
    });
  });
});
