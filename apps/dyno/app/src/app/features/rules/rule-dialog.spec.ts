import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import type { Rule } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { demoRules } from '../../core/mock/data';
import { provideIcons } from '../../shared/icons';
import { RulesStore } from '../../state/rules.store';
import { RuleDialog } from './rule-dialog';

async function setup(rule: Rule | null) {
  const close = vi.fn();
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }, { provide: DIALOG_DATA, useValue: { rule } }, { provide: DialogRef, useValue: { close } }] });
  const store = TestBed.inject(RulesStore);
  await store.load();
  const f = TestBed.createComponent(RuleDialog);
  f.detectChanges();
  const el = f.nativeElement as HTMLElement;
  const settle = async () => {
    for (let i = 0; i < 4; i++) {
      await f.whenStable();
      f.detectChanges();
    }
  };
  const type = (sel: string, value: string) => {
    const i = el.querySelector<HTMLInputElement>(sel)!;
    i.value = value;
    i.dispatchEvent(new Event('input'));
    f.detectChanges();
  };
  const click = (label: string) => {
    const b = el.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`) ?? [...el.querySelectorAll('button')].find((x) => x.textContent!.trim() === label);
    b!.click();
    f.detectChanges();
  };
  const save = () => [...el.querySelectorAll('button')].find((b) => b.textContent!.trim() === (rule ? 'Save' : 'Add rule'))!.click();
  return { f, el, store, close, settle, type, click, save, mock };
}

describe('RuleDialog', () => {
  it('creates a rule: validates early, then saves it disabled and closes', async () => {
    const { el, store, close, settle, type, click, save, f } = await setup(null);
    expect(el.textContent).toContain('Add rule');
    save();
    await settle();
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('name');
    expect(close).not.toHaveBeenCalled();

    type('#rule-name', '  Calm the build  ');
    type('[aria-label="CPU above percent"]', '55');
    type('[aria-label="For seconds"]', '90');
    type('[aria-label="Release after seconds"]', '15');
    click('Specific apps');
    type('#rule-apps', 'rustc, cargo');
    el.querySelectorAll<HTMLButtonElement>('.chips .tog')[2]!.click();
    f.detectChanges();
    const sel = el.querySelector<HTMLSelectElement>('[aria-label="Step 1 action"]')!;
    sel.value = 'cpuCap';
    sel.dispatchEvent(new Event('change'));
    f.detectChanges();
    type('[aria-label="Step 1 cap percent"]', '35');
    type('[aria-label="Step 1 delay seconds"]', '5');
    click('Add step');
    click('Move step 2 up');
    click('Remove step 2');
    save();
    await settle();
    const saved = store.ruleSet()!.rules.find((r) => r.id === 'calm-the-build')!;
    expect(saved).toMatchObject({ name: 'Calm the build', enabled: false, releaseAfterSecs: 15 });
    expect(saved.condition).toMatchObject({ cpuAbove: 55, forSecs: 90, target: { apps: ['rustc', 'cargo'] } });
    expect(saved.ladder.length).toBe(1);
    expect(saved.ladder[0]).toEqual({ action: 'efficiencyMode', afterSecs: 60 });
    expect(saved.profiles.length).toBe(1);
    expect(close).toHaveBeenCalledWith(true);
  });

  it('edits an existing rule keeping its id; the apps list can go back to any app', async () => {
    const rule = demoRules()[1]!;
    const { el, store, close, settle, type, click, save } = await setup(rule);
    expect(el.textContent).toContain('Edit rule');
    expect((el.querySelector('#rule-apps') as HTMLTextAreaElement).value).toContain('chrome.exe');
    type('#rule-name', 'Browsers, quieter');
    click('Move step 1 down');
    click('Remove step 1');
    click('Any app');
    save();
    await settle();
    const saved = store.ruleSet()!.rules.find((r) => r.id === rule.id)!;
    expect(saved.name).toBe('Browsers, quieter');
    expect(saved.condition.target).toBe('anyApp');
    expect(saved.ladder.length).toBe(1);
    expect(close).toHaveBeenCalledWith(true);
  });

  it('shows the engine message inline when it refuses, and stays open', async () => {
    const { el, close, settle, type, save, mock } = await setup(null);
    vi.spyOn(mock, 'saveRules').mockRejectedValue({ code: 'badRequest', message: 'The engine says no.' });
    type('#rule-name', 'X');
    save();
    await settle();
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('The engine says no.');
    expect(close).not.toHaveBeenCalled();
  });

  it('unselecting a profile and choosing All profiles work; Cancel closes', async () => {
    const { el, f, close } = await setup(demoRules()[0]!);
    const tags = () => [...el.querySelectorAll<HTMLButtonElement>('.chips .tog')];
    expect(tags()[0]!.getAttribute('aria-pressed')).toBe('false');
    tags()[1]!.click();
    f.detectChanges();
    expect(tags()[1]!.getAttribute('aria-pressed')).toBe('false');
    tags()[0]!.click();
    f.detectChanges();
    expect(tags()[0]!.getAttribute('aria-pressed')).toBe('true');
    [...el.querySelectorAll('button')].find((b) => b.textContent!.trim() === 'Cancel')!.click();
    expect(close).toHaveBeenCalledWith(false);
  });
});
