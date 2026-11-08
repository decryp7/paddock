import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import type { MockBackend } from '../../core/mock-backend';
import { jobsProviders, mockWithJobs } from '../../../testing/job-test-support';
import { provideIcons } from '../../shared/icons';
import { WolStore } from '../../state/wol.store';
import { WakeDevices } from './wake-devices';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

function render(options: ConstructorParameters<typeof MockBackend>[0] = {}, prepare?: (mock: MockBackend) => void) {
  const { mock, jobs } = mockWithJobs({ now: () => NOW, ...options });
  prepare?.(mock);
  TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([]), jobsProviders(jobs), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(WakeDevices);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock, store: TestBed.inject(WolStore) };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    await new Promise((r) => setTimeout(r, 2));
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
    f.detectChanges();
  }
};
const ROW = '[role="row"][data-key]';
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text))!;
const type = async (f: Parameters<typeof settle>[0], el: HTMLElement, label: string, value: string) => {
  const input = el.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!;
  input.value = value;
  input.dispatchEvent(new Event('input'));
  await settle(f);
};

describe('dy-wake-devices', () => {
  beforeEach(() => vi.useFakeTimers({ now: NOW, toFake: ['Date'] }));
  afterEach(() => vi.useRealTimers());

  it('lists the saved computers with their last wake in words, and two ways to add one', async () => {
    const { f, el } = render();
    await settle(f);
    const rows = [...el.querySelectorAll('[aria-label="Computers to wake"] ' + ROW)];
    expect(rows.map((r) => r.querySelector('strong')?.textContent)).toEqual(['Media PC', 'Office NAS']);
    expect(rows[0]!.textContent).toContain('Awake after 23 s (answered ping), 3 h');
    expect(rows[1]!.textContent).toContain('Not woken yet');
    expect(rows[1]!.textContent).toContain('SecureOn');
    expect(btn(el, 'Add from network')).toBeTruthy();
    expect(btn(el, 'Add by hand')).toBeTruthy();
    expect(el.querySelector('.sel')?.textContent).toBe('02:00:5e:10:00:01');
  });

  it('has a designed empty state that says nothing is saved until chosen', async () => {
    const { f, el } = render({ wol: 'empty' });
    await settle(f);
    expect(el.querySelector('.empty')?.textContent).toContain('No computers to wake yet');
    expect(el.querySelector('.empty')?.textContent).toContain('Nothing is saved until you choose it');
  });

  it('says plainly when Wake-on-LAN is turned off', async () => {
    const { f, el } = render({ wol: 'off' });
    await settle(f);
    expect(el.querySelector('[role="status"]')?.textContent).toContain('Wake-on-LAN is turned off in Settings');
    expect(btn(el, 'Add by hand')).toBeUndefined();
  });

  it('says why nothing can be sent from here', async () => {
    const { f, el } = render({ wol: 'cantSend' });
    await settle(f);
    expect(el.textContent).toContain('Dyno cannot send a wake from here: no network card with an IPv4 address is connected.');
  });

  it('adds a computer by hand and shows it', async () => {
    const { f, el } = render();
    await settle(f);
    btn(el, 'Add by hand').click();
    await settle(f);
    await type(f, el, 'Name of the computer', 'Garage PC');
    await type(f, el, 'Hardware address', '02:00:5e:10:00:42');
    await type(f, el, 'SecureOn password', '01:02:03:04');
    btn(el, 'Add computer').click();
    await settle(f);
    expect(el.querySelector('section[aria-label="Computer to wake"]')).toBeNull();
    const names = [...el.querySelectorAll('[aria-label="Computers to wake"] ' + ROW)].map((r) => r.querySelector('strong')?.textContent);
    expect(names).toContain('Garage PC');
  });

  it('shows the engine message in the form and keeps what was typed', async () => {
    const { f, el } = render();
    await settle(f);
    btn(el, 'Add by hand').click();
    await settle(f);
    await type(f, el, 'Name of the computer', 'Garage PC');
    await type(f, el, 'Hardware address', 'nope');
    btn(el, 'Add computer').click();
    await settle(f);
    expect(el.querySelector('mat-error')?.textContent).toContain('six pairs of hex digits');
    expect(el.querySelector('input[aria-label="Hardware address"]')?.getAttribute('aria-invalid')).toBe('true');
    expect(document.activeElement).toBe(el.querySelector('input[aria-label="Hardware address"]'));
    expect(el.querySelector<HTMLInputElement>('input[aria-label="Name of the computer"]')?.value).toBe('Garage PC');
  });

  it('adds from the network: lists unsaved computers with words for their state and fills the form from the pick', async () => {
    const { f, el } = render();
    await settle(f);
    btn(el, 'Add from network').click();
    await settle(f);
    const items = [...el.querySelectorAll('[aria-label="Computers not saved yet"] li:not(.hdr)')];
    expect(items).toHaveLength(3);
    expect(items[0]!.textContent).toContain('Answered a moment ago');
    expect(items[1]!.textContent).toContain('Seen on the network');
    expect(items[2]!.textContent).toContain('Set by hand');
    items[0]!.querySelector('button')!.click();
    await settle(f);
    expect(el.querySelector<HTMLInputElement>('input[aria-label="Hardware address"]')?.value).toBe('02:00:5e:10:00:03');
    expect(el.querySelector<HTMLInputElement>('input[aria-label="Network address"]')?.value).toBe('192.0.2.50');
  });

  it('shows an empty and an error state for the network list, with Retry', async () => {
    const empty = render({ wol: 'noNeighbours' });
    await settle(empty.f);
    btn(empty.el, 'Add from network').click();
    await settle(empty.f);
    expect(empty.el.textContent).toContain('No other computers seen yet');
  });

  it('shows the network read error with Retry', async () => {
    const { f, el } = render({ wol: 'neighboursFail' });
    await settle(f);
    btn(el, 'Add from network').click();
    await settle(f);
    expect(el.querySelector('[aria-label="Computers on your network"] [role="alert"]')?.textContent).toContain('could not be read');
    expect(btn(el, 'Retry')).toBeTruthy();
  });

  it('wakes a computer and says what was sent, then tests a wake and says it answered', async () => {
    const { f, el, store } = render();
    await settle(f);
    const row = [...el.querySelectorAll<HTMLElement>('[aria-label="Computers to wake"] ' + ROW)][0]!;
    row.querySelector<HTMLElement>('[aria-label="Wake Media PC"]')!.click();
    await settle(f);
    expect(el.querySelector('[aria-label="Wake of Media PC"]')?.textContent).toContain('Wake packets sent.');
    expect(el.querySelector('details')?.textContent).toContain('Through en0 to 255.255.255.255: 3 of 3 sent');
    await store.wake(1, true);
    await settle(f);
    expect(el.querySelector('[aria-label="Wake of Media PC"]')?.textContent).toContain('Test wake: Media PC');
    expect(el.querySelector('[aria-label="Wake of Media PC"]')?.textContent).toContain('Awake after 23 s (answered ping).');
  });

  it('a test wake that gets no answer says what to check', async () => {
    const { f, el, store } = render({ wol: 'timeout' });
    await settle(f);
    await store.wake(1, true);
    await settle(f);
    expect(el.querySelector('[aria-label="Wake of Media PC"]')?.textContent).toContain('off at the wall');
  });

  it('lists this computer\'s readiness with a word for every state, a fix for a problem and the reason when it cannot tell', async () => {
    const { f, el } = render();
    await settle(f);
    const items = [...el.querySelectorAll('.items li')];
    expect(items.map((i) => i.getAttribute('data-kind'))).toEqual(['ok', 'problem', 'cantTell', 'cantTell']);
    expect(items[0]!.textContent).toContain('Fine');
    expect(items[1]!.textContent).toContain('Needs a change');
    expect(items[1]!.textContent).toContain("Power Management tab");
    expect(items[2]!.textContent).toContain("Can't tell");
    expect(items[3]!.textContent).toContain('BIOS');
  });

  it('shows the readiness error with Retry', async () => {
    const { f, el } = render({}, (mock) => vi.spyOn(mock, 'wolReadiness').mockRejectedValueOnce({ code: 'internal', message: 'Could not read the settings.' }));
    await settle(f);
    expect(el.querySelector('section.ready [role="alert"]')?.textContent).toContain('Could not read the settings.');
  });
});
