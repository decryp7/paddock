import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { PowerPage } from './power-page';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

async function render(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const mock = new MockBackend({ now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(PowerPage);
  f.detectChanges();
  await settle(f);
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 4; i++) {
    await f.whenStable();
    await new Promise((r) => setTimeout(r, 2));
    f.detectChanges();
  }
};
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text))!;
const status = (el: HTMLElement) => el.querySelector('[aria-label="Keep awake status"]')!;
const head = (el: HTMLElement) => status(el).querySelector('h2')?.textContent;

describe('dy-power-page', () => {
  beforeEach(() => vi.useFakeTimers({ now: NOW, toFake: ['Date'] }));
  afterEach(() => vi.useRealTimers());

  it('says what the tab is for, with one subtitle per tab', async () => {
    const { f, el } = await render();
    expect(el.querySelector('h1')?.textContent).toBe('Power');
    expect(el.querySelector('.page-head p')?.textContent).toContain('Keep your computer awake when you ask');
    [...el.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent?.includes('Energy'))!.click();
    await settle(f);
    expect(el.querySelector('.page-head p')?.textContent).toContain('How long this computer was on and asleep');
  });

  it('when off, offers one primary action with the chosen length, and the durations as a segmented choice', async () => {
    const { el } = await render();
    expect(head(el)).toBe('Not keeping your computer awake');
    const presets = [...el.querySelectorAll('mat-button-toggle')].map((b) => b.textContent!.trim());
    expect(presets).toEqual(['30 minutes', '1 hour', '2 hours', '4 hours', 'Until I stop it']);
    expect(el.querySelectorAll('button[matButton="filled"]')).toHaveLength(1);
    expect(btn(el, 'Keep awake for 1 hour').getAttribute('matButton')).toBe('filled');
    expect(btn(el, 'Stop keeping awake')).toBeUndefined();
  });

  it('holds with the chosen length, says what is held and who started it, and keeps the system reference under Details', async () => {
    const { f, el } = await render();
    el.querySelectorAll<HTMLButtonElement>('mat-button-toggle button')[2]!.click();
    await settle(f);
    btn(el, 'Keep awake for 2 hours').click();
    await settle(f);
    expect(head(el)).toBe('Keeping awake, 2 h left');
    const rows = status(el).querySelector('dl')!.textContent!;
    expect(rows).toContain('Keeps awake');
    expect(rows).toContain('Computer (the display may turn off)');
    expect(rows).toContain('Started by');
    expect(rows).toContain('you, for 2 hours');
    const details = status(el).querySelector('details')!;
    expect(details.querySelector('summary')?.textContent).toBe('Details');
    expect(details.textContent).toContain('System reference');
    expect(status(el).querySelector('dl')!.textContent).not.toContain('mock');
    expect(el.querySelector('[aria-label="Start keeping awake"] h2')?.textContent).toBe('Change the hold');
    expect(el.querySelectorAll('button[matButton="filled"]')).toHaveLength(1);
    btn(el, 'Stop keeping awake').click();
    await settle(f);
    expect(head(el)).toBe('Not keeping your computer awake');
  });

  it('keeps the display on only when asked', async () => {
    const { f, el, mock } = await render();
    const spy = vi.spyOn(mock, 'powerKeepAwake');
    el.querySelector<HTMLInputElement>('mat-checkbox input')!.click();
    await settle(f);
    btn(el, 'Keep awake for 1 hour').click();
    await settle(f);
    expect(spy).toHaveBeenCalledWith({ scope: 'systemAndDisplay', until: { for: { minutes: 60 } } });
  });

  it('starts "until a time" only for a time of day and says why the button is off until then', async () => {
    const { f, el, mock } = await render();
    const spy = vi.spyOn(mock, 'powerKeepAwake');
    expect(btn(el, 'Keep awake until a time').disabled).toBe(true);
    expect(el.textContent).toContain('Pick a time first');
    const input = el.querySelector<HTMLInputElement>('input[type="time"]')!;
    input.value = '18:30';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    expect(el.textContent).not.toContain('Pick a time first');
    btn(el, 'Keep awake until 18:30').click();
    await settle(f);
    expect(spy).toHaveBeenCalledWith({ scope: 'system', until: { at: { minuteOfDay: 18 * 60 + 30 } } });
  });

  it('starts "while busy" with the ticked conditions, and with none ticked says what to do', async () => {
    const { f, el, mock } = await render();
    const spy = vi.spyOn(mock, 'powerKeepAwake');
    btn(el, 'Keep awake while busy').click();
    await settle(f);
    expect(spy).toHaveBeenCalledWith({ scope: 'system', until: { whileBusy: { conds: [{ cpu: { abovePct: 50 } }], graceSecs: 120 } } });
    const cpu = [...el.querySelectorAll<HTMLInputElement>('fieldset.busy mat-checkbox input')][0]!;
    cpu.click();
    await settle(f);
    expect(btn(el, 'Keep awake while busy').disabled).toBe(true);
    expect(el.textContent).toContain('Tick at least one condition first');
  });

  it('says what holds a busy hold now, and shows its real conditions in the form', async () => {
    const { el } = await render({ power: 'busy' });
    expect(head(el)).toBe('Keeping awake: CPU at 72%');
    expect(status(el).querySelector('h2')?.getAttribute('data-state')).toBe('holding');
    expect(status(el).querySelector('dl')!.textContent).toContain('CPU is above 50% or network is above 500 KB/s');
    const boxes = [...el.querySelectorAll<HTMLInputElement>('fieldset.busy mat-checkbox input')].map((b) => b.checked);
    expect(boxes).toEqual([true, true, false]);
    expect(el.querySelector('fieldset.busy')!.textContent).toContain('Network above 500 KB/s');
  });

  it('says a quiet busy hold is waiting for the computer to get busy and when it lets go', async () => {
    const { el } = await render({ power: 'quiet' });
    expect(head(el)).toBe('Waiting for the computer to get busy');
    expect(status(el).querySelector('h2')?.getAttribute('data-state')).toBe('waiting');
    expect(status(el).textContent).toContain('Quiet for 0:40; it lets go in 1:20 unless the computer gets busy again.');
  });

  it('shows why a hold ended and offers to keep awake again for the same length', async () => {
    const { f, el, mock } = await render({ power: 'ended' });
    expect(head(el)).toBe('Keep awake ended: Dyno restarted');
    expect(btn(el, 'Keep awake for 1 hour again')).toBeDefined();
    const spy = vi.spyOn(mock, 'powerKeepAwake');
    btn(el, 'Keep awake for 1 hour again').click();
    await settle(f);
    expect(spy).toHaveBeenCalledWith({ scope: 'system', until: { for: { minutes: 60 } } });
  });

  it('says only "Paused" as the headline of a paused hold and keeps what is held, since and ends', async () => {
    const { el } = await render({ power: 'waiting' });
    expect(head(el)).toBe('Paused');
    const text = status(el).textContent!;
    expect(text).toContain('The computer went to sleep');
    expect(text).toContain('Keeps awake');
    expect(text).toContain('you, for 1 hour');
    expect(text).toContain('Since');
    expect(text).toContain('Ends');
  });

  it('when the system cannot, says so plainly with what to do, keeps the reason under Details and shows no controls', async () => {
    const { el } = await render({ power: 'unavailable' });
    expect(head(el)).toBe("Keep awake isn't available on this system");
    expect(status(el).textContent).toContain('systemd-logind');
    expect(status(el).querySelector('details')?.textContent).toContain('no logind to ask');
    expect(el.querySelector('[aria-label="Start keeping awake"]')).toBeNull();
    expect(btn(el, 'Keep awake for')).toBeUndefined();
  });

  it('turns the controls off with the reason and a way to the settings when the feature is switched off', async () => {
    const mock = new MockBackend({ now: () => NOW });
    const settings = await mock.settings();
    await mock.saveSettings({ ...settings, power: { ...settings.power, keepAwake: { ...settings.power.keepAwake, enabled: false } } });
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(PowerPage);
    f.detectChanges();
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('[role="status"]')?.textContent).toContain('Keep awake is turned off in Settings.');
    expect(btn(el, 'Open power settings')).toBeDefined();
    expect(btn(el, 'Keep awake for 1 hour').disabled).toBe(true);
    expect(el.textContent).toContain('Keep awake is turned off in Settings.');
  });

  it('shows the engine message when a start is refused', async () => {
    const { f, el, mock } = await render();
    vi.spyOn(mock, 'powerKeepAwake').mockRejectedValueOnce({ code: 'badRequest', message: 'Keep awake for 1 minute up to 7 days.' });
    btn(el, 'Keep awake for 1 hour').click();
    await settle(f);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('Keep awake for 1 minute up to 7 days.');
  });

  it('offers the running programs to follow, by process, and says to choose one first', async () => {
    const { f, el, mock } = await render();
    expect(btn(el, 'Keep awake while it runs').disabled).toBe(true);
    expect(el.textContent).toContain('Choose a program first');
    const spy = vi.spyOn(mock, 'powerKeepAwake');
    mock.tick();
    await settle(f);
    const select = el.querySelector<HTMLSelectElement>('select')!;
    const options = [...select.options].filter((o) => o.value);
    expect(options.length).toBeGreaterThan(0);
    select.value = options[0]!.value;
    select.dispatchEvent(new Event('change'));
    await settle(f);
    btn(el, 'Keep awake while it runs').click();
    await settle(f);
    expect(spy.mock.calls[0]![0].until).toHaveProperty('whileProcess');
  });
});
