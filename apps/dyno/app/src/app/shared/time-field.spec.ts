import { TestBed } from '@angular/core/testing';
import { TimeField } from './time-field';

function make(value: string, disabled = false) {
  const f = TestBed.createComponent(TimeField);
  f.componentRef.setInput('label', 'Start of quiet hours');
  f.componentRef.setInput('value', value);
  f.componentRef.setInput('disabled', disabled);
  const emitted: string[] = [];
  f.componentInstance.timeChange.subscribe((t) => emitted.push(t));
  f.detectChanges();
  const input = (f.nativeElement as HTMLElement).querySelector<HTMLInputElement>('input[aria-label="Start of quiet hours"]')!;
  const type = (text: string) => {
    input.value = text;
    input.dispatchEvent(new Event('input'));
    f.detectChanges();
  };
  return { f, input, emitted, type, el: f.nativeElement as HTMLElement };
}

describe('TimeField', () => {
  it('shows the time in 24 hours under its label', () => {
    const { input, el } = make('22:00');
    expect(input.value).toBe('22:00');
    expect(el.querySelector('mat-label')?.textContent).toContain('Start of quiet hours');
  });

  it('emits a typed time as HH:MM', () => {
    const { type, emitted } = make('22:00');
    type('23:15');
    expect(emitted).toEqual(['23:15']);
  });

  it('keeps the last good time for text it cannot read and shows the problem once the field was left', () => {
    const { f, input, type, emitted, el } = make('22:00');
    type('bad');
    expect(emitted).toEqual([]);
    expect(el.querySelector('[data-testid="field-error"]')).toBeNull();
    input.dispatchEvent(new Event('blur'));
    f.detectChanges();
    expect(el.querySelector('[data-testid="field-error"]')?.textContent).toContain('Enter a time such as 22:00.');
  });

  it('is disabled with its toggle', () => {
    const { input, el } = make('22:00', true);
    expect(input.disabled).toBe(true);
    expect(el.querySelector<HTMLButtonElement>('mat-timepicker-toggle button')?.disabled).toBe(true);
  });
});
