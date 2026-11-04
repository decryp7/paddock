import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { SleepTab } from './sleep-tab';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

async function render(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const mock = new MockBackend({ now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(SleepTab);
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
/** Whether the "Act when idle" switch is on. */
const ruleOn = (el: HTMLElement) => el.querySelector('button[aria-label="Act when the computer is idle"]')!.getAttribute('aria-checked') === 'true';
const head = (el: HTMLElement) => el.querySelector('[aria-label="Sleep and shut down status"] h2')?.textContent;

describe('dy-sleep-tab', () => {
  beforeEach(() => vi.useFakeTimers({ now: NOW, toFake: ['Date'] }));
  afterEach(() => vi.useRealTimers());

  it('says the rules are off and that Dyno never acts unless asked', async () => {
    const { el } = await render();
    expect(head(el)).toBe('Sleep and shut down rules are off');
    expect(el.querySelector('[aria-label="Sleep and shut down status"]')?.textContent).toContain('never sleeps or shuts down');
    expect(btn(el, 'Cancel')).toBeUndefined();
    expect(btn(el, 'Save changes').disabled).toBe(true);
  });

  it('shows the countdown with Cancel and Snooze, and Cancel goes back to watching', async () => {
    const { f, el, mock } = await render({ idle: 'countdown' });
    expect(head(el)).toBe('Will sleep in 0:48');
    expect(el.textContent).toContain('Moving the mouse cancels an idle countdown');
    const spy = vi.spyOn(mock, 'powerIdleCancel');
    btn(el, 'Cancel sleep').click();
    await settle(f);
    expect(spy).toHaveBeenCalled();
    expect(head(el)).toContain('Sleep after 30 min idle');
  });

  it('snoozes for half an hour', async () => {
    const { f, el, mock } = await render({ idle: 'countdown' });
    const spy = vi.spyOn(mock, 'powerIdleSnooze');
    btn(el, 'Snooze 30 minutes').click();
    await settle(f);
    expect(spy).toHaveBeenCalledWith(30);
    expect(head(el)).toBe('Snoozed');
  });

  it('lists what is in the way, marking what Dyno cannot read', async () => {
    const { el } = await render({ idle: 'blocked' });
    expect(head(el)).toBe('Waiting: something is in the way');
    const items = [...el.querySelectorAll('.blockers li')].map((l) => l.textContent!.trim());
    expect(items[0]).toBe('Audio is playing (waiting)');
    expect(items[1]).toMatch(/^Dyno cannot tell whether another program.*\(can't tell\)$/);
    expect(el.textContent).toContain('counts as busy');
  });

  it('says what happened after the request, without retrying', async () => {
    const { el } = await render({ idle: 'stopped' });
    expect(head(el)).toBe('Shutdown did not happen: an app may be asking you to save');
    expect(el.querySelector<HTMLSelectElement>('select[aria-label="What to do when idle"]')!.value).toBe('shutDown');
    expect(el.textContent).toContain('will not try again');
  });

  it('says it is waiting for the system after the request, with a way to stop waiting', async () => {
    const { el } = await render({ idle: 'acting' });
    expect(head(el)).toBe('Waiting for the system to shut down');
    expect(btn(el, 'Stop waiting')).toBeDefined();
  });

  it('while watching says the rule and the time left, with a way to skip this idle period', async () => {
    const { f, el, mock } = await render({ idle: 'watching' });
    expect(head(el)).toBe('Sleep after 30 min idle, 23 min to go');
    expect(ruleOn(el)).toBe(true);
    const spy = vi.spyOn(mock, 'powerIdleCancel');
    btn(el, 'Cancel for this idle period').click();
    await settle(f);
    expect(spy).toHaveBeenCalled();
  });

  it('never shows a running state while the rule is switched off: the mock states come with the rule on and the action that runs', async () => {
    for (const idle of ['watching', 'blocked', 'countdown'] as const) {
      TestBed.resetTestingModule();
      const { el } = await render({ idle });
      expect(ruleOn(el), idle).toBe(true);
    }
    TestBed.resetTestingModule();
    const { el } = await render({ idle: 'acting' });
    expect(head(el)).toBe('Waiting for the system to shut down');
    expect(el.querySelector<HTMLSelectElement>('select[aria-label="What to do when idle"]')!.value).toBe('shutDown');
    expect(el.textContent).not.toContain('A choice the system cannot do here says why.');
  });

  it('says why the idle rule cannot work, and the timed actions still do', async () => {
    const { el } = await render({ idle: 'unavailable' });
    expect(head(el)).toBe('The idle rule cannot work on this computer');
    expect(el.textContent).toContain('timed actions below still work');
  });

  it('shows the warning for the chosen action and saves the rule with the engine as the judge', async () => {
    const { f, el, mock } = await render();
    expect(el.textContent).toContain('Dyno asks your computer to sleep the normal way');
    const toggle = el.querySelector<HTMLInputElement>('[aria-label="Act when the computer is idle"]')!;
    toggle.click();
    await settle(f);
    const save = vi.spyOn(mock, 'saveSettings');
    expect(btn(el, 'Save changes').disabled).toBe(false);
    btn(el, 'Save changes').click();
    await settle(f);
    expect(save).toHaveBeenCalled();
    const saved = save.mock.calls[0]![0].power.idle;
    expect(saved.enabled).toBe(true);
    expect(saved.acOnly).toBe(true);
    expect(saved.exceptions.otherStayAwake).toBe(true);
  });

  it('turns a choice the system cannot do into a disabled option with its reason', async () => {
    const { el } = await render({ power: 'unavailable' });
    const options = [...el.querySelectorAll<HTMLOptionElement>('select[aria-label="What to do when idle"] option')];
    const hibernate = options.find((o) => o.textContent!.includes('Hibernate'))!;
    expect(hibernate.disabled).toBe(true);
    expect(hibernate.textContent).toContain('decides by itself when to hibernate');
  });

  it('adds and removes a timed action and keeps the engine message when a save is refused', async () => {
    const { f, el, mock } = await render();
    expect(el.textContent).toContain('No timed actions yet');
    const time = el.querySelector<HTMLInputElement>('input[type="time"]')!;
    time.value = '23:00';
    time.dispatchEvent(new Event('input'));
    await settle(f);
    btn(el, 'Add').click();
    await settle(f);
    expect(el.querySelector('table[aria-label="Timed actions"] tbody tr')?.textContent).toContain('23:00');
    expect(el.querySelector('table tbody tr')?.textContent).toContain('Every day');
    vi.spyOn(mock, 'saveSettings').mockRejectedValueOnce({ code: 'badRequest', message: 'Pick a time of day between 00:00 and 23:59.' });
    btn(el, 'Save changes').click();
    await settle(f);
    expect(el.querySelector('.problem')?.textContent).toContain('Pick a time of day');
    btn(el, 'Remove').click();
    await settle(f);
    expect(el.textContent).toContain('No timed actions yet');
  });
});
