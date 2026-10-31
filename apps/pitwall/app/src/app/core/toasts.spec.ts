import { TestBed } from '@angular/core/testing';
import { NOTIFICATION_SOURCES, PdNotificationHost } from '@decryp7/paddock-ui/shell/notify';
import { Toasts } from './toasts';

function setup() {
  TestBed.configureTestingModule({ providers: [{ provide: NOTIFICATION_SOURCES, multi: true, useExisting: Toasts }] });
  const fixture = TestBed.createComponent(PdNotificationHost);
  fixture.detectChanges();
  return { fixture, el: fixture.nativeElement as HTMLElement, toasts: TestBed.inject(Toasts) };
}

describe('Toasts in the family toast host', () => {
  it('maps each tone to a severity, one cause per toast', () => {
    const toasts = TestBed.inject(Toasts);
    toasts.show('Saved', 'ok');
    toasts.show('Saved', 'ok');
    toasts.show('Broke', 'bad', 0);
    toasts.show('FYI');
    const items = toasts.items();
    expect(items.map((n) => n.severity)).toEqual(['success', 'success', 'error', 'info']);
    expect(new Set(items.map((n) => n.cause)).size).toBe(4);
    expect(items[0]).toMatchObject({ title: 'Saved', ttl: 4000 });
    expect(items[2]).toMatchObject({ sticky: true });
  });

  it('keeps the four newest', () => {
    const toasts = TestBed.inject(Toasts);
    for (const m of ['a', 'b', 'c', 'd', 'e']) toasts.show(m);
    expect(toasts.toasts().map((t) => t.message)).toEqual(['b', 'c', 'd', 'e']);
  });

  it('renders the toasts in the host, errors as alerts', () => {
    const { fixture, el, toasts } = setup();
    toasts.show('Saved', 'ok');
    toasts.error({ code: 'serverUnavailable', message: 'Server is down' });
    fixture.detectChanges();
    const shown = Array.from(el.querySelectorAll('[data-testid="notification"]'));
    expect(shown.map((t) => t.getAttribute('data-severity'))).toEqual(['success', 'error']);
    expect(shown[1]!.getAttribute('role')).toBe('alert');
    expect(el.textContent).toContain('Server is down');
  });

  it('a cancelled action is information, not an error', () => {
    const toasts = TestBed.inject(Toasts);
    toasts.error({ code: 'cancelled', message: 'Cancelled.' });
    expect(toasts.items()[0]!.severity).toBe('info');
  });

  it('the dismiss button forgets the toast', () => {
    const { fixture, el, toasts } = setup();
    toasts.show('First', 'info', 0);
    toasts.show('Second', 'info', 0);
    fixture.detectChanges();
    (el.querySelector('[data-testid="notification-dismiss"]') as HTMLButtonElement).click();
    fixture.detectChanges();
    expect(toasts.toasts().map((t) => t.message)).toEqual(['Second']);
  });
});
