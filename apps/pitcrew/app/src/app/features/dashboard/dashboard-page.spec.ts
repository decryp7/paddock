import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { RegisterLauncher } from '../../shell/register-launcher';
import { appProviders, byText, flush, makeMock, settle, text } from '../../../testing/test-helpers';
import { DashboardPage } from './dashboard-page';

describe('DashboardPage', () => {
  let mock: MockBackend;
  const register = { open: vi.fn() };

  function configure(options: Parameters<typeof makeMock>[0] = {}) {
    localStorage.clear();
    mock = makeMock(options);
    TestBed.configureTestingModule({ providers: [...appProviders(mock), { provide: RegisterLauncher, useValue: register }] });
  }
  afterEach(() => {
    mock.dispose();
    register.open.mockClear();
  });

  async function mount() {
    const f = TestBed.createComponent(DashboardPage);
    await settle(f, 6);
    return { f, el: f.nativeElement as HTMLElement };
  }

  it('shows the summary, a tile per runner and the history cards', async () => {
    configure();
    const { el } = await mount();
    expect(text(el.querySelector('.subtitle'))).toBe('4 runners · 1 running a job · 1 needs attention');
    expect(el.querySelectorAll('pc-runner-tile')).toHaveLength(4);
    for (const title of ['Runner load', 'History', 'CPU', 'Memory', 'Jobs per day', 'Events', 'Uptime', 'Average job duration', 'Queue wait', 'Updates', 'Runner folders']) {
      expect(text(el)).toContain(title);
    }
    expect(el.querySelectorAll('pc-chart').length).toBeGreaterThanOrEqual(5);
    expect(el.querySelector('pc-timeline')).not.toBeNull();
    expect(el.querySelector('pc-machine-tiles')).not.toBeNull();
    expect(text(el.querySelector('.updates'))).toContain('Updated');
    expect(text(el)).toContain('Outcomes of');
    expect(text(el.querySelector('pc-events-feed'))).not.toBe('');
  });

  it('shows jobs per hour in the 24 h range', async () => {
    configure();
    const { f, el } = await mount();
    const page = f.componentInstance as unknown as { store: { setRange(r: '24h' | '7d' | '30d'): Promise<void> } };
    await page.store.setRange('24h');
    await settle(f, 6);
    expect(text(el)).toContain('Jobs per hour');
    await page.store.setRange('7d');
    await settle(f, 6);
    expect(text(el)).toContain('Jobs per day');
  });

  it('loads details of the running job into its tile', async () => {
    configure();
    const { el } = await mount();
    expect(text(el.querySelector('pc-runner-tile .run'))).toContain('Retry declined card payments');
  });

  it('changes the range', async () => {
    configure();
    const activity = vi.spyOn(mock, 'activity');
    const { f, el } = await mount();
    const radios = Array.from(el.querySelectorAll('[role="radio"]')) as HTMLButtonElement[];
    expect(radios.map((r) => text(r))).toEqual(['24 h', '7 days', '30 days']);
    expect(radios[1]!.getAttribute('aria-checked')).toBe('true');
    radios[0]!.click();
    await settle(f);
    expect(radios[0]!.getAttribute('aria-checked')).toBe('true');
    expect(activity).toHaveBeenCalledWith('build-win', 2);
    expect(text(el)).toContain('last 24 h');
  });

  it('focuses the history on one runner', async () => {
    configure();
    const { f, el } = await mount();
    const select = el.querySelector('select') as HTMLSelectElement;
    select.value = 'ui-tests';
    select.dispatchEvent(new Event('change'));
    await settle(f);
    expect(text(el.querySelector('.track'))).toBe('');
    expect(el.querySelectorAll('.track')).toHaveLength(1);
    expect(el.querySelector('.track')?.getAttribute('aria-label')).toContain('ui-tests');
    select.value = '';
    select.dispatchEvent(new Event('change'));
    await settle(f);
    expect(el.querySelectorAll('.track')).toHaveLength(4);
  });

  it('keeps the period control in the History header, and only the primary in the page header', async () => {
    configure();
    const { el } = await mount();
    expect(el.querySelector('header.page-header [role="radiogroup"]')).toBeNull();
    expect(el.querySelectorAll('header.page-header button')).toHaveLength(1);
    expect(el.querySelector('#history-title')?.parentElement?.querySelector('[role="radiogroup"]')).not.toBeNull();
  });

  it('opens the register wizard', async () => {
    configure();
    const { el } = await mount();
    byText<HTMLButtonElement>(el, 'header button', 'Register runner')!.click();
    expect(register.open).toHaveBeenCalled();
  });

  it('invites to register a first runner when there are none', async () => {
    configure({ scenario: 'empty' });
    const { el } = await mount();
    expect(text(el)).toContain('No runners yet');
    byText<HTMLButtonElement>(el, 'pc-empty-state button', 'Register a runner')!.click();
    expect(register.open).toHaveBeenCalled();
    expect(text(el.querySelector('.subtitle'))).toBe('No runners yet');
  });

  it('waits for the service when it is unreachable', async () => {
    configure({ scenario: 'unavailable' });
    const { el } = await mount();
    expect(text(el)).toContain('Waiting for the Pitcrew service');
    expect(text(el.querySelector('.subtitle'))).toBe('Not connected to the Pitcrew service');
  });

  it('shows skeletons while connecting', async () => {
    configure({ latencyMs: 5_000 });
    const f = TestBed.createComponent(DashboardPage);
    f.detectChanges();
    await flush();
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('.subtitle'))).toBe('Connecting to the Pitcrew service…');
    expect(el.querySelectorAll('.skel-tile')).toHaveLength(2);
  });

  it('shows an error with Retry when history cannot load, then recovers', async () => {
    configure();
    vi.spyOn(mock, 'metrics').mockRejectedValue({ code: 'internal', message: 'metrics broke' });
    const { f, el } = await mount();
    expect(text(el)).toContain('Machine metrics could not be loaded');
    expect(text(el)).toContain('metrics broke');
    vi.mocked(mock.metrics).mockRestore();
    byText<HTMLButtonElement>(el, 'pc-load-state button', 'Retry')!.click();
    await settle(f, 6);
    expect(text(el)).not.toContain('Machine metrics could not be loaded');
    expect(el.querySelector('pc-machine-tiles')).not.toBeNull();
  });

  it('shows a failed request as an inline error with Retry instead of skeletons forever', async () => {
    configure();
    vi.spyOn(mock, 'activity').mockRejectedValue({ code: 'serviceUnavailable', message: 'The service is not running.' });
    const { f, el } = await mount();
    expect(text(el)).toContain('Activity could not be loaded');
    expect(text(el)).toContain('The service is not running.');
    vi.mocked(mock.activity).mockRestore();
    byText<HTMLButtonElement>(el, 'pc-load-state button', 'Retry')!.click();
    await settle(f, 6);
    expect(text(el)).not.toContain('Activity could not be loaded');
  });


  it('refreshes on a timer while visible', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] });
    try {
      configure();
      const f = TestBed.createComponent(DashboardPage);
      await settle(f);
      const activity = vi.spyOn(mock, 'activity');
      vi.advanceTimersByTime(61_000);
      await settle(f);
      expect(activity).toHaveBeenCalled();
      f.destroy();
    } finally {
      vi.useRealTimers();
    }
  });
});
