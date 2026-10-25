import { TestBed } from '@angular/core/testing';
import type { HelperStatus } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Notifier } from '../../shared/notifier';
import { HelperGate } from './helper-gate';

function setup(prepare: (mock: MockBackend) => void = () => undefined) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  prepare(mock);
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, gate: TestBed.inject(HelperGate), notifier: TestBed.inject(Notifier) };
}
const settle = async () => {
  for (let i = 0; i < 6; i++) await new Promise((r) => setTimeout(r, 0));
};
const status = (state: HelperStatus['state'], extra: Partial<HelperStatus> = {}): HelperStatus => ({ state, message: `The helper is ${state}.`, version: null, prompt: 'The system asks for your password.', recent: [], error: null, ...extra });
const dialogText = () => (document.querySelector('dy-helper-dialog') as HTMLElement | null)?.textContent?.replace(/\s+/g, ' ') ?? '';
const press = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('dy-helper-dialog button')].find((b) => b.textContent?.trim() === label)!.click();

describe('HelperGate', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = '')));

  it('lets a change through without asking when the helper is ready', async () => {
    const { gate, mock } = setup();
    await mock.installHelper();
    expect(await gate.ensure()).toBe(true);
    expect(document.querySelector('dy-helper-dialog')).toBeNull();
  });

  it('explains first and installs only on the confirm click; Not now returns false and installs nothing', async () => {
    const { gate, mock } = setup();
    const install = vi.spyOn(mock, 'installHelper');
    const declined = gate.ensure();
    await settle();
    expect(dialogText()).toContain('Install the Dyno helper');
    expect(dialogText()).toContain('The system asks you to confirm');
    expect(install).not.toHaveBeenCalled();
    press('Not now');
    expect(await declined).toBe(false);
    expect(install).not.toHaveBeenCalled();

    const accepted = gate.ensure();
    await settle();
    press('Install the helper');
    expect(await accepted).toBe(true);
    expect(install).toHaveBeenCalledTimes(1);
  });

  it('shows the reason and Try again when the prompt was dismissed or the install failed', async () => {
    const { gate, mock } = setup();
    const install = vi.spyOn(mock, 'installHelper').mockResolvedValue(status('notInstalled', { error: 'The permission prompt was closed, so nothing was installed.' }));
    const pending = gate.ensure();
    await settle();
    press('Install the helper');
    await settle();
    expect(dialogText()).toContain('The permission prompt was closed');
    install.mockResolvedValue(status('ready', { version: '1.0.0' }));
    press('Try again');
    expect(await pending).toBe(true);
  });

  it('an update or repair says so in the title and the button', async () => {
    for (const [state, title, button] of [
      ['outdated', 'Update the Dyno helper', 'Update the helper'],
      ['unreachable', 'Repair the Dyno helper', 'Repair the helper'],
    ] as const) {
      TestBed.resetTestingModule();
      const { gate } = setup((m) => vi.spyOn(m, 'helperStatus').mockResolvedValue(status(state)));
      const pending = gate.ensure();
      await settle();
      expect(dialogText()).toContain(title);
      expect(dialogText()).toContain(button);
      press('Not now');
      expect(await pending).toBe(false);
    }
  });

  it('where the helper cannot be installed it says why and offers only Close', async () => {
    const { gate } = setup((m) => vi.spyOn(m, 'helperStatus').mockResolvedValue(status('unsupported', { message: 'pkexec (polkit) is not installed.' })));
    const pending = gate.ensure();
    await settle();
    expect(dialogText()).toContain('pkexec (polkit) is not installed.');
    expect([...document.querySelectorAll('dy-helper-dialog footer button')].map((b) => b.textContent?.trim())).toEqual(['Close']);
    press('Close');
    expect(await pending).toBe(false);
  });

  it('when the agent cannot even say, it reports the error and returns false', async () => {
    const { gate, notifier } = setup((m) => vi.spyOn(m, 'helperStatus').mockRejectedValue({ code: 'agentUnavailable', message: 'The agent is not running.' }));
    expect(await gate.ensure()).toBe(false);
    expect(notifier.toasts()[0]?.text).toBe('The agent is not running.');
    expect(document.querySelector('dy-helper-dialog')).toBeNull();
  });
});
