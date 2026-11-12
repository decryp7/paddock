import { TestBed } from '@angular/core/testing';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell/contributions';
import { DiagnosticsPage } from './diagnostics-page';

describe('dy-diagnostics-page', () => {
  function render() {
    const openDialog = vi.fn().mockResolvedValue(undefined);
    TestBed.configureTestingModule({ providers: [{ provide: SHELL_PORTS, useValue: { showPage: vi.fn(), openDialog, notify: vi.fn() } }] });
    const f = TestBed.createComponent(DiagnosticsPage);
    f.detectChanges();
    return { el: f.nativeElement as HTMLElement, openDialog };
  }

  it('has one button and it opens the one problem report dialog', () => {
    const { el, openDialog } = render();
    expect(el.querySelector('h1')?.textContent).toBe('Diagnostics bundle');
    const buttons = el.querySelectorAll('button');
    expect(buttons).toHaveLength(1);
    (buttons[0] as HTMLButtonElement).click();
    expect(openDialog).toHaveBeenCalledExactlyOnceWith('pd.report-problem');
  });

  it('says issue reports are public and what may remain', () => {
    const { el } = render();
    const text = el.textContent ?? '';
    expect(text).toContain('Issue reports are public');
    expect(text).toContain('File names outside your home folder may remain');
    expect(el.querySelectorAll('.never li')).toHaveLength(4);
  });
});
