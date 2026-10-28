import { TestBed } from '@angular/core/testing';
import { DIALOG_INPUT, DIALOG_REF } from '@decryp7/paddock-ui/shell/dialogs';
import { ConfirmDialog } from './confirm-dialog';

function render(danger: boolean): HTMLElement {
  const input = { title: 'Delete watch', message: 'Delete Lumen CLI?', confirmLabel: 'Delete', danger };
  TestBed.configureTestingModule({
    providers: [
      { provide: DIALOG_INPUT, useValue: input },
      { provide: DIALOG_REF, useValue: { close: () => undefined } },
    ],
  });
  const fixture = TestBed.createComponent(ConfirmDialog);
  fixture.detectChanges();
  return fixture.nativeElement as HTMLElement;
}

describe('the sp.confirm decision', () => {
  it('makes the way forward the default when nothing is lost', () => {
    const el = render(false);
    expect(el.querySelector('[data-default]')?.textContent?.trim()).toBe('Delete');
  });

  it('makes Cancel the default (focus and Enter) when the action is dangerous', () => {
    const el = render(true);
    const defaults = el.querySelectorAll('[data-default]');
    expect(defaults).toHaveLength(1);
    expect(defaults[0]?.textContent?.trim()).toBe('Cancel');
  });
});
