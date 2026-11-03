import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { EnergyTab } from './energy-tab';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

async function render(configure: (m: MockBackend) => Promise<void> = async () => {}) {
  const mock = new MockBackend({ now: () => NOW });
  await configure(mock);
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(EnergyTab);
  f.detectChanges();
  for (let i = 0; i < 4; i++) {
    await f.whenStable();
    await new Promise((r) => setTimeout(r, 2));
    f.detectChanges();
  }
  return { f, el: f.nativeElement as HTMLElement, mock };
}

describe('dy-energy-tab', () => {
  beforeEach(() => vi.useFakeTimers({ now: NOW, toFake: ['Date'] }));
  afterEach(() => vi.useRealTimers());

  it('says there are no figures until the person enters watts, and still shows the time', async () => {
    const { el } = await render();
    expect(el.querySelector('[role="status"]')?.textContent).toContain('Settings > Power');
    expect(el.querySelectorAll('tbody tr')).toHaveLength(7);
    expect(el.textContent).toContain('Not seen');
    expect(el.querySelector('tbody td:nth-child(5)')?.textContent).toBe('Not set');
    expect([...el.querySelectorAll('[aria-label="Totals for the range"] small')].some((s) => s.textContent === 'Estimated')).toBe(false);
  });

  it('shows the estimate, the cost with the currency and the totals when watts are set', async () => {
    const { el } = await render(async (m) => {
      const s = await m.settings();
      await m.saveSettings({ ...s, power: { ...s.power, energy: { ...s.power.energy, wattsOn: 60, wattsSleep: 2, tariffPerKwh: 0.5, currency: 'S$', co2GPerKwh: 400 } } });
    });
    expect(el.querySelector('[role="status"]')?.textContent).toContain('Estimated from your numbers');
    expect(el.querySelector('[aria-label="Totals for the range"]')?.textContent).toMatch(/kWh/);
    expect(el.querySelector('tbody td:nth-child(6)')?.textContent).toMatch(/^S\$ \d+\.\d\d$/);
    expect(el.querySelector('[aria-label="Totals for the range"]')?.textContent).toMatch(/kg/);
    expect([...el.querySelectorAll('[aria-label="Totals for the range"] small')].filter((s) => s.textContent === 'Estimated')).toHaveLength(3);
    expect(el.querySelector('.inputs')?.textContent).toContain('From your numbers: 60 W on, 2 W asleep, S$ 0.50/kWh, 0.40 kg CO₂/kWh');
    expect(el.querySelector('.inputs')?.textContent).toContain('Edit in Settings');
    expect(el.querySelector('table tbody tr td:nth-child(7)')?.textContent).toMatch(/kg/);
  });

  it('draws one stacked bar per day with axes in hours, and lists the newest day first', async () => {
    const { el } = await render();
    expect(el.querySelectorAll('svg rect.on')).toHaveLength(7);
    expect(el.querySelector('svg[role="img"]')?.getAttribute('aria-label')).toBe('Hours on and asleep for each of the last 7 days');
    expect([...el.querySelectorAll('svg text.tick')].map((t) => t.textContent)).toEqual(expect.arrayContaining(['0 h', '12 h', '24 h']));
    expect(el.querySelector('svg text.axis')?.textContent).toBe('Hours per day');
    const first = el.querySelector('tbody tr th')?.textContent;
    const last = el.querySelector('tbody tr:last-child th')?.textContent;
    expect(new Date(NOW).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' })).toBe(first);
    expect(first).not.toBe(last);
  });

  it('opens the power settings from the page', async () => {
    const { el } = await render();
    expect([...el.querySelectorAll('button')].some((b) => b.textContent?.includes('Open power settings'))).toBe(true);
  });

  it('says so when the log is off', async () => {
    const { el } = await render(async (m) => {
      const s = await m.settings();
      await m.saveSettings({ ...s, power: { ...s.power, energy: { ...s.power.energy, enabled: false } } });
    });
    expect(el.textContent).toContain('The energy log is off');
    expect(el.querySelector('table')).toBeNull();
  });

  it('asks again for another range', async () => {
    const { f, el, mock } = await render();
    const spy = vi.spyOn(mock, 'energyReport');
    const toggles = [...el.querySelectorAll<HTMLButtonElement>('mat-button-toggle button')];
    toggles.find((b) => b.textContent?.includes('30 days'))!.click();
    for (let i = 0; i < 3; i++) {
      await f.whenStable();
      f.detectChanges();
    }
    expect(spy).toHaveBeenCalledWith(30);
    expect(el.querySelectorAll('tbody tr')).toHaveLength(30);
  });

  it('keeps the engine message and offers Retry when the report fails', async () => {
    const mock = new MockBackend({ now: () => NOW });
    vi.spyOn(mock, 'energyReport').mockRejectedValueOnce({ code: 'internal', message: 'Dyno could not read its power log.' });
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(EnergyTab);
    f.detectChanges();
    for (let i = 0; i < 4; i++) {
      await f.whenStable();
      f.detectChanges();
    }
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('Dyno could not read its power log.');
    [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Retry'))!.click();
    for (let i = 0; i < 4; i++) {
      await f.whenStable();
      f.detectChanges();
    }
    expect(el.querySelectorAll('tbody tr').length).toBeGreaterThan(0);
  });
});
