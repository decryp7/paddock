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
  template: `<label for="theme">Theme</label><pd-select inputId="theme" [options]="options" [value]="value()" placeholder="Choose" [disabled]="disabled()" (valueChange)="value.set($event)" />`,
})
class Host {
  readonly options = OPTIONS;
  readonly value = signal('dark');
  readonly disabled = signal(false);
}

async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  await f.whenStable();
  TestBed.tick();
  await f.whenStable();
}

describe('pd-select', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = '')));

  function setup() {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    return f;
  }
  const trigger = (f: { nativeElement: HTMLElement }) => f.nativeElement.querySelector<HTMLButtonElement>('button.pd-trigger')!;
  const options = () => [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=option]')];

  it('shows the chosen label, is named by its label, and lists nothing until opened', async () => {
    const f = setup();
    await settle(f);
    expect(trigger(f).textContent?.trim()).toBe('Dark');
    expect(trigger(f).getAttribute('role')).toBe('combobox');
    expect(trigger(f).id).toBe('theme');
    expect(trigger(f).getAttribute('aria-expanded')).toBe('false');
    expect(options()).toHaveLength(0);
  });

  it('opens a themed list with the options (the chosen one marked), and a click chooses and closes it', async () => {
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
    expect(options()).toHaveLength(0);
    expect(trigger(f).getAttribute('aria-expanded')).toBe('false');
    expect(document.activeElement).toBe(trigger(f));
  });

  it('opens with the arrow keys, closes on Escape and returns focus; choosing the same value emits nothing', async () => {
    const f = setup();
    await settle(f);
    trigger(f).focus();
    trigger(f).dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    await settle(f);
    expect(options()).toHaveLength(4);
    document.querySelector('.cdk-overlay-container [role=listbox]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await settle(f);
    expect(options()).toHaveLength(0);
    expect(document.activeElement).toBe(trigger(f));
    trigger(f).click();
    await settle(f);
    options()[1]!.click();
    await settle(f);
    expect(f.componentInstance.value()).toBe('dark');
  });

  it('a click outside closes the list without choosing', async () => {
    const f = setup();
    await settle(f);
    trigger(f).click();
    await settle(f);
    document.body.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle(f);
    expect(options()).toHaveLength(0);
    expect(f.componentInstance.value()).toBe('dark');
  });

  it('a disabled select does not open, and a value outside the options shows the placeholder', async () => {
    const f = setup();
    f.componentInstance.disabled.set(true);
    await settle(f);
    trigger(f).click();
    await settle(f);
    expect(options()).toHaveLength(0);
    f.componentInstance.disabled.set(false);
    f.componentInstance.value.set('gone');
    await settle(f);
    expect(trigger(f).textContent?.trim()).toBe('Choose');
  });
});
