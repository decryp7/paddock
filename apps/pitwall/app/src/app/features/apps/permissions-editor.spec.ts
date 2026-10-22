import { TestBed } from '@angular/core/testing';
import type { AppPermissions, LocalModel } from '../../core/api.types';
import { PermissionsEditor, parseLimit, samePermissions } from './permissions-editor';

const BASE: AppPermissions = {
  models: null,
  maxPriority: 'interactive',
  tools: true,
  extendedApis: false,
  limits: { requestsPerMin: 60, tokensPerDay: null, maxConcurrent: 2, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null },
};

const MODELS = [
  { id: 'm1', name: 'Model one', quant: 'Q4_K_M' },
  { id: 'm2', name: 'Model two', quant: null },
] as LocalModel[];

function render(p: AppPermissions = BASE) {
  const fixture = TestBed.createComponent(PermissionsEditor);
  fixture.componentRef.setInput('permissions', p);
  fixture.componentRef.setInput('models', MODELS);
  fixture.componentRef.setInput('appName', 'Debrief');
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  const saved: AppPermissions[] = [];
  fixture.componentInstance.save.subscribe((v) => saved.push(v));
  const btn = (text: string) => Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.trim().startsWith(text))!;
  const input = (sel: string) => el.querySelector(sel) as HTMLInputElement;
  const type = (i: HTMLInputElement, v: string) => {
    i.value = v;
    i.dispatchEvent(new Event('input'));
    fixture.detectChanges();
  };
  return { fixture, el, saved, btn, input, type };
}

describe('parseLimit', () => {
  it('reads empty as no limit, whole positive numbers as limits, the rest as invalid', () => {
    expect(parseLimit('')).toBeNull();
    expect(parseLimit('  ')).toBeNull();
    expect(parseLimit('60')).toBe(60);
    expect(parseLimit(' 7 ')).toBe(7);
    expect(parseLimit('0')).toBe('invalid');
    expect(parseLimit('-3')).toBe('invalid');
    expect(parseLimit('1.5')).toBe('invalid');
    expect(parseLimit('abc')).toBe('invalid');
    expect(parseLimit('99999999999999999999')).toBe('invalid');
    expect(parseLimit('100', 100)).toBe(100);
    expect(parseLimit('101', 100)).toBe('invalid');
  });
});

describe('samePermissions', () => {
  it('ignores model order and compares everything else', () => {
    expect(samePermissions({ ...BASE, models: ['a', 'b'] }, { ...BASE, models: ['b', 'a'] })).toBe(true);
    expect(samePermissions(BASE, { ...BASE, models: [] })).toBe(false);
    expect(samePermissions(BASE, { ...BASE, tools: false })).toBe(false);
    expect(samePermissions(BASE, { ...BASE, limits: { ...BASE.limits, maxConcurrent: null, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null } })).toBe(false);
  });
});

describe('PermissionsEditor', () => {
  it('starts clean and saves only after a change', () => {
    const { fixture, el, saved, btn } = render();
    expect(btn('Save').disabled).toBe(true);
    expect((el.querySelector('input[type="radio"]') as HTMLInputElement).checked).toBe(true);
    btn('Batch').click();
    fixture.detectChanges();
    expect(btn('Save').disabled).toBe(false);
    btn('Save').click();
    expect(saved).toEqual([{ ...BASE, maxPriority: 'batch' }]);
  });

  it('chooses models, parses limits and blocks saving invalid limits', () => {
    const { fixture, el, saved, btn, input, type } = render();
    const radios = el.querySelectorAll<HTMLInputElement>('input[type="radio"]');
    radios[1]!.dispatchEvent(new Event('change'));
    fixture.detectChanges();
    const boxes = el.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    expect(boxes.length).toBe(2);
    boxes[1]!.dispatchEvent(new Event('change'));
    boxes[0]!.dispatchEvent(new Event('change'));
    boxes[0]!.dispatchEvent(new Event('change'));
    fixture.detectChanges();
    const rpm = input('input[id$="-requestsPerMin"]');
    type(rpm, 'lots');
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('whole number');
    expect(btn('Save').disabled).toBe(true);
    type(rpm, '');
    type(input('input[id$="-tokensPerDay"]'), '50000');
    btn('Save').click();
    expect(saved).toEqual([{ ...BASE, models: ['m2'], limits: { requestsPerMin: null, tokensPerDay: 50000, maxConcurrent: 2, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null } }]);
  });

  it('edits the daily, monthly and memory-share limits and caps the share at 100 percent', () => {
    const { fixture, el, saved, btn, input, type } = render();
    type(input('input[id$="-maxMemoryPercent"]'), '101');
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('from 1 to 100');
    expect(btn('Save').disabled).toBe(true);
    type(input('input[id$="-maxMemoryPercent"]'), '40');
    type(input('input[id$="-requestsPerDay"]'), '2000');
    type(input('input[id$="-tokensPerMonth"]'), '9000000');
    fixture.detectChanges();
    btn('Save').click();
    expect(saved[0]!.limits).toEqual({ requestsPerMin: 60, tokensPerDay: null, maxConcurrent: 2, requestsPerDay: 2000, tokensPerMonth: 9_000_000, maxMemoryPercent: 40 });
  });

  it('resets local edits to the saved permissions', () => {
    const { fixture, btn, input, type } = render();
    type(input('input[id$="-maxConcurrent"]'), '9');
    expect(btn('Reset').disabled).toBe(false);
    btn('Reset').click();
    fixture.detectChanges();
    expect(input('input[id$="-maxConcurrent"]').value).toBe('2');
    expect(btn('Save').disabled).toBe(true);
  });

  it('explains when there are no local models to choose from', () => {
    const { fixture, el } = render({ ...BASE, models: [] });
    fixture.componentRef.setInput('models', []);
    fixture.detectChanges();
    expect(el.textContent).toContain('No local models yet');
  });
});
