import { TestBed } from '@angular/core/testing';
import { Switch } from './switch';

function make(checked: boolean, disabled = false) {
  const f = TestBed.createComponent(Switch);
  f.componentRef.setInput('checked', checked);
  f.componentRef.setInput('label', 'Alerts');
  f.componentRef.setInput('disabled', disabled);
  const emitted: boolean[] = [];
  f.componentInstance.changed.subscribe((v) => emitted.push(v));
  f.detectChanges();
  const button = (f.nativeElement as HTMLElement).querySelector('button')!;
  return { f, button, emitted };
}

describe('Switch', () => {
  it('is an accessible switch reflecting its state', () => {
    const { button } = make(true);
    expect(button.getAttribute('role')).toBe('switch');
    expect(button.getAttribute('aria-checked')).toBe('true');
    expect(button.getAttribute('aria-label')).toBe('Alerts');
    expect(make(false).button.getAttribute('aria-checked')).toBe('false');
  });
  it('emits the opposite value when clicked', () => {
    const on = make(true);
    on.button.click();
    expect(on.emitted).toEqual([false]);
    const off = make(false);
    off.button.click();
    expect(off.emitted).toEqual([true]);
  });
  it('does not emit while disabled', () => {
    const { button, emitted } = make(false, true);
    expect(button.disabled).toBe(true);
    button.click();
    expect(emitted).toEqual([]);
  });
});
