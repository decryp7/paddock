import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MatTimepicker, MatTimepickerInput } from '@angular/material/timepicker';
import { By } from '@angular/platform-browser';
import { describe, expect, it } from 'vitest';
import { PdKeepTimeOnEscape } from './keep-time';
import { TIME_PROVIDERS, timeToDate } from './time-value';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatTimepicker, MatTimepickerInput, PdKeepTimeOnEscape],
  providers: [TIME_PROVIDERS],
  template: `<input id="t" [matTimepicker]="picker" pdKeepTime [value]="value()" /><mat-timepicker #picker />`,
})
class Host {
  readonly value = signal<Date | null>(timeToDate('09:30'));
}

function make() {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
  const f = TestBed.createComponent(Host);
  f.detectChanges();
  const de = f.debugElement.query(By.css('#t'));
  return { f, input: de.nativeElement as HTMLInputElement, picker: de.injector.get(MatTimepickerInput<Date>) };
}

const escape = (input: HTMLInputElement) => {
  const e = new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true, cancelable: true });
  input.dispatchEvent(e);
  return e;
};

describe('PdKeepTimeOnEscape (pdKeepTime)', () => {
  it('keeps the time when Escape is pressed in a field whose list is shut', () => {
    const { input, picker } = make();
    escape(input);
    expect(input.value).toBe('09:30');
    expect(picker.value()?.getHours()).toBe(9);
  });

  it('leaves the key unhandled for what is around the field (a dialog closes on it)', () => {
    const { input } = make();
    let above: { defaultPrevented: boolean; keyCode: number } | null = null;
    input.parentElement!.addEventListener('keydown', (e) => (above = { defaultPrevented: e.defaultPrevented, keyCode: (e as KeyboardEvent).keyCode }));
    escape(input);
    expect(above).toEqual({ defaultPrevented: false, keyCode: 27 });
  });

  it('does not touch other keys', () => {
    const { input } = make();
    const e = new KeyboardEvent('keydown', { key: 'a', keyCode: 65, bubbles: true, cancelable: true });
    input.dispatchEvent(e);
    expect(e.keyCode).toBe(65);
  });
});
