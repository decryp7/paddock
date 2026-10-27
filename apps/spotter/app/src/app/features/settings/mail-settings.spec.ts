import { TestBed } from '@angular/core/testing';
import { SettingsPage } from './settings-page';
import { SettingsStore } from '../../state/settings.store';
import { appProviders, byText, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';

async function make(backend = makeMock()) {
  localStorage.clear();
  TestBed.configureTestingModule({ providers: appProviders(backend) });
  const f = TestBed.createComponent(SettingsPage);
  await settle(f);
  return { f, backend, store: TestBed.inject(SettingsStore) };
}
type F = Awaited<ReturnType<typeof make>>['f'];
const field = (f: F, name: string) => f.nativeElement.querySelector(`sp-mail-settings input[name="${name}"]`) as HTMLInputElement;
const btn = (f: F, label: string) => byText(f.nativeElement.querySelector('sp-mail-settings') as HTMLElement, 'button', label) as HTMLButtonElement;
const fill = async (f: F, values: Record<string, string>) => {
  for (const [k, v] of Object.entries(values)) typeInto(field(f, k), v);
  await settle(f);
};
const submit = async (f: F) => {
  (f.nativeElement.querySelector('sp-mail-settings form') as HTMLFormElement).dispatchEvent(new Event('submit', { cancelable: true }));
  await settle(f);
};

describe('Settings e-mail form', () => {
  it('starts empty with Save and the test disabled', async () => {
    const { f } = await make();
    expect(text(f.nativeElement.querySelector('sp-mail-settings'))).toContain('Set up a mail server');
    expect(btn(f, 'Save').disabled).toBe(true);
    expect(btn(f, 'Send test e-mail').disabled).toBe(true);
    expect(f.nativeElement.querySelector('sp-mail-settings input[type=password]')).not.toBeNull();
  });

  it('saves the server, keeps the password in the credential store and then allows a test', async () => {
    const { f, backend, store } = await make();
    await fill(f, { host: 'smtp.example.org', from: 'spotter@example.org', username: 'robot', password: 's3cret' });
    expect(btn(f, 'Save').disabled).toBe(false);
    await submit(f);
    expect((await backend.getSettings()).smtp).toEqual({ host: 'smtp.example.org', port: 587, security: 'start-tls', username: 'robot', from: 'spotter@example.org' });
    expect(backend.calls).toContain('setMailPassword');
    expect(store.mail()).toEqual({ configured: true, passwordSet: true });
    expect(field(f, 'password').value).toBe('');
    expect(field(f, 'password').placeholder).toContain('Saved');
    expect(text(f.nativeElement.querySelector('sp-mail-settings'))).toContain('Mail settings saved.');
    await fill(f, { to: 'me@example.org' });
    await (async () => { btn(f, 'Send test e-mail').click(); await settle(f); })();
    expect(text(f.nativeElement.querySelector('sp-mail-settings [role=status]'))).toBe('Test e-mail sent to me@example.org.');
  });

  it('moves the port with the security mode unless the person typed their own', async () => {
    const { f } = await make();
    const tab = (label: string) => byText(f.nativeElement, '.seg-tabs[aria-label="Mail security"] button', new RegExp(`^${label}$`)) as HTMLButtonElement;
    tab('TLS').click();
    await settle(f);
    expect(field(f, 'port').value).toBe('465');
    await fill(f, { port: '2525' });
    tab('None').click();
    await settle(f);
    expect(field(f, 'port').value).toBe('2525');
    expect(tab('None').getAttribute('aria-pressed')).toBe('true');
  });

  it('shows the typed error from a failed test inline, in an alert', async () => {
    const { f } = await make();
    await fill(f, { host: 'smtp.example.org', from: 'spotter@example.org' });
    await submit(f);
    await fill(f, { to: 'not an address' });
    btn(f, 'Send test e-mail').click();
    await settle(f);
    const alert = f.nativeElement.querySelector('sp-mail-settings [role=alert]') as HTMLElement;
    expect(text(alert)).toContain('Enter an e-mail address');
  });

  it('shows a refused save inline and removes the saved password on request', async () => {
    const { f, backend, store } = await make();
    await fill(f, { host: 'smtp.example.org', from: 'spotter@example.org', password: 'pw' });
    await submit(f);
    btn(f, 'Remove saved password').click();
    await settle(f);
    expect(store.mail()?.passwordSet).toBe(false);
    vi.spyOn(backend, 'saveSettings').mockRejectedValueOnce({ code: 'invalid', message: 'The mail server needs a sender.', problems: ['From is empty'] });
    await fill(f, { from: '' });
    await submit(f);
    const alert = text(f.nativeElement.querySelector('sp-mail-settings [role=alert]'));
    expect(alert).toContain('The mail server needs a sender.');
    expect(alert).toContain('From is empty');
  });

  it('clears the mail settings when the server is emptied', async () => {
    const { f, backend } = await make();
    await fill(f, { host: 'smtp.example.org', from: 'a@example.org' });
    await submit(f);
    await fill(f, { host: '' });
    await submit(f);
    expect((await backend.getSettings()).smtp).toBeNull();
    expect(text(f.nativeElement.querySelector('sp-mail-settings'))).toContain('Mail settings cleared.');
  });

  it('does not reset typing when another setting is saved', async () => {
    const { f, store } = await make();
    await fill(f, { host: 'draft.example.org' });
    await store.change({ notifications: false });
    await settle(f);
    expect(field(f, 'host').value).toBe('draft.example.org');
  });
});
