import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { ServiceSection } from './service-section';

describe('ServiceSection', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  async function mount() {
    mock = makeMock({ scenario: 'unavailable' });
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(ServiceSection);
    await settle(f, 6);
    return { f, el: f.nativeElement as HTMLElement };
  }

  it('shows the repair and, when the item is off in Login Items, a button that opens the pane instead of installing', async () => {
    mock = makeMock({ scenario: 'unavailable' });
    vi.spyOn(mock, 'serviceInstall').mockResolvedValue({
      platform: 'macos',
      installed: false,
      running: false,
      canInstall: true,
      message: null,
      repair: { state: 'failed', message: 'Open System Settings ▸ General ▸ Login Items and switch Pitcrew off and on again.' },
      disabledInLoginItems: true,
      startFailed: false,
      notInLoginItems: false,
    });
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(ServiceSection);
    await settle(f, 6);
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('[data-testid="service-repair"]'))).toContain('switch Pitcrew off and on again');
    expect(text(el.querySelector('[data-testid="service-disabled"]'))).toContain('turned off in System Settings');
    const buttons = Array.from(el.querySelectorAll('button')).map((b) => text(b));
    expect(buttons).not.toContain('Install service');
    el.querySelector<HTMLButtonElement>('[data-testid="service-disabled"] button')!.click();
    await settle(f);
    expect(mock.loginItemsOpened).toBe(1);
  });

  it('with the service down it still renders, offers Repair on macOS and calls the app command', async () => {
    mock = makeMock({ scenario: 'demo', serverDown: true, startFailed: true });
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(ServiceSection);
    await settle(f, 6);
    const el = f.nativeElement as HTMLElement;
    expect(text(el)).toContain('Installed, not running');
    expect(text(el.querySelector('[data-testid="service-start-failed"]'))).toContain('could not start after the update');
    const repair = Array.from(el.querySelectorAll('button')).find((b) => text(b) === 'Repair service')!;
    repair.click();
    await settle(f, 6);
    expect(mock.repairs).toBe(1);
    expect(text(el)).toContain('Installed and running');
    expect(el.querySelector('[data-testid="service-start-failed"]')).toBeNull();
  });

  it('shows no notices when nothing is wrong', async () => {
    const { el } = await mount();
    expect(el.querySelector('[data-testid="service-repair"]')).toBeNull();
    expect(el.querySelector('[data-testid="service-disabled"]')).toBeNull();
  });
});
