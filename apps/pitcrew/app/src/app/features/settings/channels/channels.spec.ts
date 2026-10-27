import { Dialog } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../../core/backend';
import type { MockBackend } from '../../../core/mock-backend';
import { appProviders, byText, flush, makeMock, settle, text, type as typeInto } from '../../../../testing/test-helpers';
import { KIND_GROUPS, blankChannel, newChannelId, secretKeyFor, setKinds, splitAddresses, withSecretKey } from './channels-model';
import { ChannelsSection } from './channels-section';
import { ChannelsStore } from './channels.store';

describe('channels model', () => {
  it('generates unique ch- ids', () => {
    expect(newChannelId([])).toMatch(/^ch-[0-9a-f]{8}$/);
    const seq = [0, 0, 0, 0, 0, 0, 0, 0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
    let i = 0;
    expect(newChannelId(['ch-00000000'], () => seq[i++]!)).toBe('ch-88888888');
  });

  it('splits addresses and toggles kinds', () => {
    expect(splitAddresses('a@x.io, b@x.io;\n c@x.io ,')).toEqual(['a@x.io', 'b@x.io', 'c@x.io']);
    expect(setKinds(['a'], ['b', 'c'], true)).toEqual(['a', 'b', 'c']);
    expect(setKinds(['a', 'b'], ['b'], false)).toEqual(['a']);
  });

  it('points the transport at the secret only when one is stored', () => {
    const c = blankChannel('ch-1');
    expect(withSecretKey(c, true).transport).toMatchObject({ secretKey: 'channel-ch-1' });
    expect(withSecretKey(c, false).transport).toMatchObject({ secretKey: null });
    expect(secretKeyFor('ch-1')).toBe('channel-ch-1');
  });
});

describe('ChannelsSection', () => {
  let mock: MockBackend;
  beforeEach(() => {
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
  });
  afterEach(() => {
    TestBed.inject(Dialog).closeAll();
    mock.dispose();
    TestBed.resetTestingModule();
  });

  async function mount() {
    const f = TestBed.createComponent(ChannelsSection);
    await settle(f);
    return { f, el: f.nativeElement as HTMLElement };
  }
  const input = (el: HTMLElement, id: string) => el.querySelector<HTMLInputElement>('#' + id)!;
  const btn = (el: HTMLElement, label: string | RegExp) => byText<HTMLButtonElement>(el, 'button', label)!;

  async function addWebhook(f: Awaited<ReturnType<typeof mount>>['f'], el: HTMLElement, url = 'https://hooks.example.test/x') {
    btn(el, 'Add channel').click();
    await settle(f);
    typeInto(input(el, 'ch-name'), 'Team chat');
    typeInto(input(el, 'ch-url'), url);
    f.detectChanges();
  }

  it('shows the empty state with an Add channel button', async () => {
    const { el } = await mount();
    expect(text(el)).toContain('No channels. Pitcrew only shows desktop notifications.');
    expect(btn(el, 'Add channel')).toBeTruthy();
  });

  it('adds a webhook channel that starts disabled with the needs-action events', async () => {
    const { f, el } = await mount();
    await addWebhook(f, el);
    expect(el.querySelector('mat-slide-toggle [role="switch"]')?.getAttribute('aria-checked')).toBe('false');
    expect(input(el, 'ch-secret').type).toBe('password');
    expect(el.querySelectorAll('.check input').length).toBe(16);
    expect(btn(el, 'Send test message')).toBeFalsy();
    btn(el, 'Save').click();
    await settle(f);
    const saved = (await mock.channels()).channels;
    expect(saved).toHaveLength(1);
    expect(saved[0]).toMatchObject({ name: 'Team chat', enabled: false, minSeverity: 'warning' });
    expect(saved[0]!.id).toMatch(/^ch-[0-9a-f]{8}$/);
    expect(saved[0]!.events).toEqual(KIND_GROUPS[0]!.kinds);
    expect(text(el)).toContain('Team chat');
  });

  it('shows only the fields of the chosen type', async () => {
    const { f, el } = await mount();
    btn(el, 'Add channel').click();
    await settle(f);
    const select = input(el, 'ch-type') as unknown as HTMLSelectElement;
    typeInto(select, 'ntfy', 'change');
    f.detectChanges();
    expect(input(el, 'ch-url')).toBeNull();
    expect(input(el, 'ch-topic')).not.toBeNull();
    typeInto(select, 'email', 'change');
    f.detectChanges();
    expect(input(el, 'ch-host')).not.toBeNull();
    expect(text(el)).toContain('SMTP password');
    typeInto(input(el, 'ch-to'), 'a@x.io, b@x.io', 'change');
    typeInto(input(el, 'ch-port'), '465', 'change');
    f.detectChanges();
    expect((input(el, 'ch-to')).value).toBe('a@x.io, b@x.io');
  });

  it('stores the secret in the keychain only, never in the settings or the DOM', async () => {
    const { f, el } = await mount();
    await addWebhook(f, el);
    typeInto(input(el, 'ch-secret'), 'tok-123');
    f.detectChanges();
    btn(el, 'Save').click();
    await settle(f);
    const saved = (await mock.channels()).channels[0]!;
    expect(JSON.stringify(saved)).not.toContain('tok-123');
    expect(saved.transport).toMatchObject({ secretKey: secretKeyFor(saved.id) });
    expect(await mock.hasChannelSecret(secretKeyFor(saved.id))).toBe(true);
    btn(el, 'Edit').click();
    await settle(f);
    expect(text(el)).toContain('Stored in the OS keychain');
    expect(input(el, 'ch-secret')).toBeNull();
    expect(el.innerHTML).not.toContain('tok-123');
  });

  it('replaces and removes a stored secret', async () => {
    const { f, el } = await mount();
    await addWebhook(f, el);
    typeInto(input(el, 'ch-secret'), 'tok-1');
    btn(el, 'Save').click();
    await settle(f);
    btn(el, 'Edit').click();
    await settle(f);
    btn(el, 'Replace').click();
    await settle(f);
    expect(input(el, 'ch-secret').value).toBe('');
    typeInto(input(el, 'ch-secret'), 'tok-2');
    btn(el, 'Save').click();
    await settle(f);
    expect(input(el, 'ch-secret')).toBeNull();
    btn(el, 'Edit').click();
    await settle(f);
    btn(el, 'Remove').click();
    await settle(f);
    const id = (await mock.channels()).channels[0]!.id;
    expect(await mock.hasChannelSecret(secretKeyFor(id))).toBe(false);
    expect(input(el, 'ch-secret')).not.toBeNull();
    btn(el, 'Save').click();
    await settle(f);
    expect((await mock.channels()).channels[0]!.transport).toMatchObject({ secretKey: null });
  });

  it('selects and clears a whole group of events', async () => {
    const { f, el } = await mount();
    btn(el, 'Add channel').click();
    await settle(f);
    byText<HTMLButtonElement>(el, 'button[aria-label="Select all in Updates"]', 'All')!.click();
    f.detectChanges();
    byText<HTMLButtonElement>(el, 'button[aria-label="Select none in Needs action"]', 'None')!.click();
    f.detectChanges();
    const checked = Array.from(el.querySelectorAll<HTMLInputElement>('.check input')).filter((i) => i.checked).length;
    expect(checked).toBe(3);
    const stalled = byText<HTMLElement>(el, '.check', 'A runner took jobs again')!.querySelector('input')!;
    stalled.click();
    f.detectChanges();
    expect(stalled.checked).toBe(true);
  });

  it('shows the validation error of the backend inline', async () => {
    const { f, el } = await mount();
    await addWebhook(f, el, 'ftp://nope');
    btn(el, 'Save').click();
    await settle(f);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('must start with http');
    expect((await mock.channels()).channels).toHaveLength(0);
  });

  it('sends a test message for a saved channel, and reports a failure', async () => {
    const { f, el } = await mount();
    await addWebhook(f, el);
    btn(el, 'Save').click();
    await settle(f);
    btn(el, 'Edit').click();
    await settle(f);
    btn(el, 'Send test message').click();
    await settle(f);
    expect(text(el)).toContain('Test sent: Sent. Check the destination.');
    vi.spyOn(mock, 'testChannel').mockRejectedValue({ code: 'badRequest', message: 'connection refused' });
    btn(el, 'Send test message').click();
    await settle(f);
    expect(text(el)).toContain('Test failed: connection refused');
  });

  it('deletes a channel and its secret after confirming', async () => {
    const { f, el } = await mount();
    await addWebhook(f, el);
    typeInto(input(el, 'ch-secret'), 'tok');
    btn(el, 'Save').click();
    await settle(f);
    const id = (await mock.channels()).channels[0]!.id;
    btn(el, 'Edit').click();
    await settle(f);
    btn(el, 'Delete channel').click();
    await flush();
    for (const b of Array.from(document.querySelectorAll<HTMLButtonElement>('.pc-dialog-panel button'))) if (b.textContent?.includes('Cancel')) b.click();
    await settle(f);
    expect((await mock.channels()).channels).toHaveLength(1);
    btn(el, 'Delete channel').click();
    await flush();
    const ok = Array.from(document.querySelectorAll<HTMLButtonElement>('.pc-dialog-panel button')).find((b) => b.textContent?.includes('Delete channel'))!;
    ok.click();
    await settle(f);
    expect((await mock.channels()).channels).toHaveLength(0);
    expect(await mock.hasChannelSecret(secretKeyFor(id))).toBe(false);
    expect(text(el)).toContain('No channels.');
  });
});

describe('ChannelsStore', () => {
  it('keeps no secret value and reports load errors', async () => {
    const mock = makeMock();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
    const store = TestBed.inject(ChannelsStore);
    vi.spyOn(mock, 'channels').mockRejectedValueOnce({ code: 'serviceUnavailable', message: 'down' });
    await store.load();
    expect(store.channels().error?.code).toBe('serviceUnavailable');
    expect(await store.test('nope')).toMatchObject({ ok: false });
    expect(store.hasSecret('nope')).toBe(false);
    mock.dispose();
  });
});
