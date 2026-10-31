import { By } from '@angular/platform-browser';
import { TestBed } from '@angular/core/testing';
import { PdWidgetChooser } from '@decryp7/paddock-ui/widgets';
import { TrayWidgetsPage } from './tray-widgets-page';
import { appProviders, byText, makeMock, settle, text } from '../../../testing/test-helpers';

async function make(backend = makeMock()) {
  TestBed.configureTestingModule({ providers: appProviders(backend) });
  const f = TestBed.createComponent(TrayWidgetsPage);
  await settle(f);
  return { f, backend };
}

describe('Tray popup settings page', () => {
  afterEach(() => vi.restoreAllMocks());

  it('shows the shared chooser on the engine layout, and the live preview of the popup beside it', async () => {
    const { f } = await make();
    const chooser = f.debugElement.query(By.directive(PdWidgetChooser));
    expect(chooser).not.toBeNull();
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('h1'))).toBe('Tray popup');
    expect(el.querySelectorAll('.pd-wc-item').length).toBeGreaterThanOrEqual(5);
    const preview = el.querySelector('pd-tray-popover[aria-label="Preview of the popup"]');
    expect(preview?.querySelectorAll('pd-widget-frame')).toHaveLength(5);
  });

  it('sends an edit from the chooser to the engine and redraws from its answer, the preview with it', async () => {
    const { f, backend } = await make();
    const chooser = f.debugElement.query(By.directive(PdWidgetChooser)).componentInstance as PdWidgetChooser<unknown, string, string>;
    chooser.edit.emit({ remove: { id: 'failures' } } as never);
    await settle(f);
    expect(backend.calls).toContain('editPopover');
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('pd-tray-popover pd-widget-frame')).not.toBeNull();
    expect(el.querySelectorAll('pd-tray-popover pd-widget-frame')).toHaveLength(4);
    expect(text(el)).not.toContain('Watches that could not be checked, with Retry Remove');
  });

  it('adds a widget of a kind with the kind as the widget (Spotter has no widget options)', async () => {
    const { f, backend } = await make();
    const spy = vi.spyOn(backend, 'editPopover');
    const chooser = f.debugElement.query(By.directive(PdWidgetChooser)).componentInstance as PdWidgetChooser<unknown, string, string>;
    chooser.edit.emit({ add: { widget: 'quietHours', span: null, at: null } } as never);
    await settle(f);
    expect(spy).toHaveBeenCalledWith({ add: { widget: 'quietHours', span: null, at: null } });
    expect(f.nativeElement.querySelectorAll('pd-tray-popover pd-widget-frame')).toHaveLength(6);
  });

  it('says why the engine refused an edit and keeps the layout', async () => {
    const { f } = await make();
    const chooser = f.debugElement.query(By.directive(PdWidgetChooser)).componentInstance as PdWidgetChooser<unknown, string, string>;
    chooser.edit.emit({ add: { widget: 'failures', span: null, at: null } } as never);
    await settle(f);
    expect(text(f.nativeElement.querySelector('.problem[role=alert]'))).toContain('already shown');
    expect(f.nativeElement.querySelectorAll('pd-tray-popover pd-widget-frame')).toHaveLength(5);
  });

  it('shows a loading panel, then the failure with Retry when the engine does not answer', async () => {
    const backend = makeMock();
    const real = backend.popoverView.bind(backend);
    let fail = true;
    vi.spyOn(backend, 'popoverView').mockImplementation(() => (fail ? Promise.reject({ code: 'unavailable', message: 'Engine is down.', problems: [] }) : real()));
    const { f } = await make(backend);
    expect(text(f.nativeElement.querySelector('[role=alert]'))).toContain('Engine is down.');
    fail = false;
    (byText(f.nativeElement, 'button', 'Retry') as HTMLButtonElement).click();
    await settle(f);
    expect(f.nativeElement.querySelector('pd-widget-chooser')).not.toBeNull();
  });
});
