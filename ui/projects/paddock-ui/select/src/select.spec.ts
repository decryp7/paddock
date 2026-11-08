import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it } from 'vitest';
import { PdSelect, type PdSelectOption } from './select';

const OPTIONS: PdSelectOption[] = [
  { value: 'system', label: 'System' },
  { value: 'dark', label: 'Dark' },
  { value: 'light', label: 'Light' },
  { value: 'off', label: 'Off', disabled: true },
];

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSelect],
  template: `<label for="theme">Theme</label><pd-select inputId="theme" [options]="options" [value]="value()" placeholder="Choose" [disabled]="disabled()" [invalid]="invalid()" describedBy="theme-hint" (valueChange)="value.set($event)" />`,
})
class Host {
  readonly options = OPTIONS;
  readonly value = signal('dark');
  readonly disabled = signal(false);
  readonly invalid = signal(false);
}

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSelect],
  template: `<pd-select ariaLabel="Unit" [options]="options" value="light" /><pd-select inputId="nolabel" [options]="options" />`,
})
class Bare {
  readonly options = OPTIONS;
}

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSelect],
  template: `<pd-select ariaLabel="A" [options]="options" /><pd-select ariaLabel="B" [options]="options" [panelWidth]="null" />`,
})
class Sized {
  readonly options = OPTIONS;
}

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSelect],
  template: `<pd-select label="Theme" [options]="options" value="dark" />`,
})
class Labelled {
  readonly options = OPTIONS;
}

async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  await f.whenStable();
  TestBed.tick();
  await f.whenStable();
}

const key = (el: Element, k: string, keyCode: number) => el.dispatchEvent(new KeyboardEvent('keydown', { key: k, keyCode, bubbles: true, cancelable: true }));

describe('pd-select', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = '')));

  function setup() {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    return f;
  }
  const trigger = (f: { nativeElement: HTMLElement }) => f.nativeElement.querySelector<HTMLElement>('mat-select')!;
  const options = () => [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=option]')];

  it('is an outline Material field showing the chosen label, named by its label, with nothing listed until opened', async () => {
    const f = setup();
    await settle(f);
    await settle(f);
    expect(f.nativeElement.querySelector('.mat-mdc-form-field.mat-form-field-appearance-outline')).not.toBeNull();
    expect(trigger(f).textContent?.trim()).toBe('Dark');
    expect(trigger(f).getAttribute('role')).toBe('combobox');
    expect(trigger(f).id).toBe('theme');
    expect(trigger(f).getAttribute('aria-expanded')).toBe('false');
    const label = f.nativeElement.querySelector('label')!;
    // the label is not ours: no id is written onto it, its text names the select
    expect(label.id).toBe('');
    expect(trigger(f).getAttribute('aria-label')).toBe('Theme');
    expect(trigger(f).getAttribute('aria-describedby')).toBe('theme-hint');
    expect(options()).toHaveLength(0);
  });

  it('opens a list with the options (the chosen one selected), and a click chooses and closes it', async () => {
    const f = setup();
    await settle(f);
    trigger(f).click();
    await settle(f);
    expect(trigger(f).getAttribute('aria-expanded')).toBe('true');
    expect(options().map((o) => o.textContent?.trim())).toEqual(['System', 'Dark', 'Light', 'Off']);
    expect(options()[1]!.getAttribute('aria-selected')).toBe('true');
    expect(options()[3]!.getAttribute('aria-disabled')).toBe('true');
    options()[2]!.click();
    await settle(f);
    expect(f.componentInstance.value()).toBe('light');
    // (the panel leaves the DOM when its close animation ends, which jsdom never fires)
    expect(trigger(f).getAttribute('aria-expanded')).toBe('false');
  });

  it('opens and moves with the arrow keys, chooses with Enter, closes on Escape; choosing the same value emits nothing', async () => {
    const f = setup();
    await settle(f);
    trigger(f).focus();
    // closed, the arrows choose the next option (Material's behaviour); Enter opens the list
    key(trigger(f), 'ArrowDown', 40);
    await settle(f);
    expect(f.componentInstance.value()).toBe('light');
    key(trigger(f), 'Enter', 13);
    await settle(f);
    expect(trigger(f).getAttribute('aria-expanded')).toBe('true');
    expect(options()).toHaveLength(4);
    key(trigger(f), 'ArrowUp', 38);
    key(trigger(f), 'Enter', 13);
    await settle(f);
    expect(f.componentInstance.value()).toBe('dark');
    expect(trigger(f).getAttribute('aria-expanded')).toBe('false');
    key(trigger(f), 'Enter', 13);
    await settle(f);
    key(trigger(f), 'Escape', 27);
    await settle(f);
    expect(trigger(f).getAttribute('aria-expanded')).toBe('false');
    expect(f.componentInstance.value()).toBe('dark');
    trigger(f).click();
    await settle(f);
    options()[1]!.click();
    await settle(f);
    expect(f.componentInstance.value()).toBe('dark');
  });

  it('Escape closes the open list and stops there (an enclosing dialog must stay open); with the list shut it goes on', async () => {
    const f = setup();
    const seen: string[] = [];
    (f.nativeElement as HTMLElement).addEventListener('keydown', (e) => seen.push((e as KeyboardEvent).key));
    await settle(f);
    trigger(f).click();
    await settle(f);
    expect(trigger(f).getAttribute('aria-expanded')).toBe('true');
    key(trigger(f), 'Escape', 27);
    await settle(f);
    expect(trigger(f).getAttribute('aria-expanded')).toBe('false');
    expect(seen).toEqual([]);
    key(trigger(f), 'Escape', 27);
    expect(seen).toEqual(['Escape']);
  });

  it('marks the chosen option with a check', async () => {
    const f = setup();
    await settle(f);
    trigger(f).click();
    await settle(f);
    expect(options()[1]!.querySelector('.mat-pseudo-checkbox-minimal')).not.toBeNull();
    expect(options()[0]!.querySelector('.mat-pseudo-checkbox-minimal')).toBeNull();
  });

  it('a click outside closes the list without choosing', async () => {
    const f = setup();
    await settle(f);
    trigger(f).click();
    await settle(f);
    document.querySelector<HTMLElement>('.cdk-overlay-backdrop')!.click();
    await settle(f);
    expect(trigger(f).getAttribute('aria-expanded')).toBe('false');
    expect(f.componentInstance.value()).toBe('dark');
  });

  it('a disabled select does not open, and a value outside the options shows the placeholder', async () => {
    const f = setup();
    f.componentInstance.disabled.set(true);
    await settle(f);
    expect(trigger(f).getAttribute('aria-disabled')).toBe('true');
    trigger(f).click();
    await settle(f);
    expect(trigger(f).getAttribute('aria-expanded')).toBe('false');
    f.componentInstance.disabled.set(false);
    f.componentInstance.value.set('gone');
    await settle(f);
    expect(trigger(f).textContent?.trim()).toBe('Choose');
  });

  it('shows its own label in the outline and is named by it', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Labelled);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('label')!.textContent).toContain('Theme');
    expect(el.querySelector('mat-select')!.getAttribute('aria-labelledby')).toContain(el.querySelector('label')!.id);
  });

  it('shows the error state when invalid', async () => {
    const f = setup();
    await settle(f);
    expect(f.nativeElement.querySelector('.mat-form-field-invalid')).toBeNull();
    f.componentInstance.invalid.set(true);
    await settle(f);
    expect(f.nativeElement.querySelector('.mat-form-field-invalid')).not.toBeNull();
  });

  it('keeps its aria-label, and an inputId without a label stays unnamed by labels', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Bare);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    await settle(f);
    const [named, bare] = [...(f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('mat-select')];
    expect(named!.getAttribute('aria-label')).toBe('Unit');
    expect(named!.textContent?.trim()).toBe('Light');
    expect(bare!.id).toBe('nolabel');
    expect(bare!.getAttribute('aria-labelledby') ?? '').not.toContain('-label');
  });

  it('sizes its list to the trigger by default and to its longest option when panelWidth is null', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Sized);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    await settle(f);
    const [byTrigger, byOptions] = (f.debugElement.children as { componentInstance: PdSelect }[]).map((c) => c.componentInstance);
    expect(byTrigger!.panelWidth()).toBe('auto');
    expect(byOptions!.panelWidth()).toBeNull();
  });
});
