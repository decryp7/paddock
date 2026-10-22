import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { Toasts } from '../../core/toasts';
import { registerIcons } from '../icons';
import { ToastHost } from './toast-host';

function setup() {
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const fixture = TestBed.createComponent(ToastHost);
  fixture.detectChanges();
  return { fixture, el: fixture.nativeElement as HTMLElement, toasts: TestBed.inject(Toasts) };
}

describe('ToastHost', () => {
  it('is a polite live region and empty without toasts', () => {
    const { el } = setup();
    const stack = el.querySelector('.stack')!;
    expect(stack.getAttribute('role')).toBe('status');
    expect(stack.getAttribute('aria-live')).toBe('polite');
    expect(el.querySelectorAll('.toast').length).toBe(0);
  });

  it('renders each toast with its tone and message', () => {
    const { fixture, el, toasts } = setup();
    toasts.show('Saved', 'ok', 0);
    toasts.show('Broke', 'bad', 0);
    toasts.show('FYI', 'info', 0);
    fixture.detectChanges();
    const items = Array.from(el.querySelectorAll('.toast'));
    expect(items.map((t) => t.getAttribute('data-tone'))).toEqual(['ok', 'bad', 'info']);
    expect(items.map((t) => t.querySelector('span')!.textContent)).toEqual(['Saved', 'Broke', 'FYI']);
    expect(items.map((t) => t.querySelector('mat-icon')!.getAttribute('data-mat-icon-name'))).toEqual(['success', 'error', 'info']);
  });

  it('dismisses a toast with its button', () => {
    const { fixture, el, toasts } = setup();
    toasts.show('First', 'info', 0);
    toasts.show('Second', 'info', 0);
    fixture.detectChanges();
    (el.querySelectorAll('button[aria-label="Dismiss"]')[0] as HTMLButtonElement).click();
    fixture.detectChanges();
    expect(Array.from(el.querySelectorAll('.toast span')).map((s) => s.textContent)).toEqual(['Second']);
  });

  it('shows action errors as bad toasts', () => {
    const { fixture, el, toasts } = setup();
    toasts.error({ code: 'serverUnavailable', message: 'Server is down' });
    fixture.detectChanges();
    expect(el.querySelector('.toast')!.getAttribute('data-tone')).toBe('bad');
    expect(el.textContent).toContain('Server is down');
  });
});
