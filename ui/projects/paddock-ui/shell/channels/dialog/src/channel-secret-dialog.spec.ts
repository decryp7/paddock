import { MemoryChannelsBackend, provideChannelsBackend } from '@decryp7/paddock-ui/shell/channels';
import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { PdChannelSecretDialog } from './channel-secret-dialog';

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

describe('the channel secret dialog', () => {
  async function open(inputs: { channelId?: string; label?: string; kind?: 'token' | 'password' } = {}, memory = new MemoryChannelsBackend()) {
    const given: Record<string, unknown> = { ...inputs };
    if (inputs.channelId === undefined) given['channelId'] = 'slack';
    const d = await renderFramed(PdChannelSecretDialog, { id: 'pd.channels.secret', kind: 'form', modality: 'modal', inputs: given, providers: [provideChannelsBackend({ backend: () => memory })] });
    const input = (): HTMLInputElement => d.q<HTMLInputElement>('channel-secret-input')!;
    const typeSecret = async (value: string): Promise<void> => {
      input().value = value;
      input().dispatchEvent(new Event('input', { bubbles: true }));
      await d.stable();
    };
    return { ...d, el: d.el, input, typeSecret, memory };
  }

  it('asks for a token with a masked field that does not autocomplete, focused, and names the channel in the title', async () => {
    const d = await open({ label: 'Team Slack' });
    expect(d.input().type).toBe('password');
    expect(d.input().getAttribute('autocomplete')).toBe('off');
    expect(d.input().getAttribute('spellcheck')).toBe('false');
    expect(document.activeElement).toBe(d.input());
    expect(text(d.el.querySelector('label')!)).toContain('Token');
    expect(text(d.el)).toContain('The token for Team Slack');
    expect(d.title()).toBe('Token for Team Slack');
    expect(d.actions()).toEqual(['dismiss', 'save']);
    expect(d.action('save')!.disabled).toBe(true);
    expect(d.action('save')!.className).toContain('pd-frame-btn--filled');
  });

  it('asks for a password for an email channel, falling back to the id as the name', async () => {
    const d = await open({ kind: 'password', channelId: 'mail' });
    expect(text(d.el.querySelector('label')!)).toContain('Password');
    expect(d.title()).toBe('Password for mail');
  });

  it('shows and hides what was typed', async () => {
    const d = await open();
    const toggle = d.q('channel-secret-toggle')!;
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    expect(toggle.getAttribute('aria-label')).toBe('Show what you typed');
    toggle.click();
    await d.stable();
    expect(d.input().type).toBe('text');
    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    expect(toggle.getAttribute('aria-label')).toBe('Hide what you typed');
  });

  it('sends the secret straight to the engine on Enter, clears it and answers true', async () => {
    const d = await open();
    await d.typeSecret('s3cret');
    expect(d.action('save')!.disabled).toBe(false);
    await d.key('Enter');
    expect(d.memory.secretOf('slack')).toBe('s3cret');
    expect(d.outcome()).toEqual({ kind: 'answered', value: true });
    expect(document.body.innerHTML).not.toContain('s3cret');
  });

  it('says Saving… on the busy Save button while the engine stores the secret', async () => {
    let release!: () => void;
    class Slow extends MemoryChannelsBackend {
      override setSecret(): Promise<void> {
        return new Promise((resolve) => (release = resolve));
      }
    }
    const d = await open({}, new Slow());
    await d.typeSecret('s3cret');
    expect(d.action('save')!.textContent!.trim()).toBe('Save');
    void d.press('save');
    await d.stable();
    expect(d.action('save')!.textContent!.trim()).toBe('Saving…');
    release();
    await d.stable();
  });

  it('shows the engine’s refusal in the frame’s alert, keeps the dialog open and the secret for Try again', async () => {
    class Refusing extends MemoryChannelsBackend {
      override setSecret(): Promise<void> {
        return Promise.reject({ code: 'storeUnavailable', message: 'The keychain is locked.', retryable: true });
      }
    }
    const d = await open({}, new Refusing());
    await d.typeSecret('s3cret');
    await d.press('save');
    expect(d.outcome()).toBeNull();
    expect(d.q('dialog-alert')!.getAttribute('role')).toBe('alert');
    expect(text(d.q('dialog-alert-text')!)).toBe('The keychain is locked.');
    expect(d.input().value).toBe('s3cret');
    expect(d.action('save')!.disabled).toBe(false);
  });

  it('opened without a channel it says so inline with a Close, and shows no field to type a secret in', async () => {
    const d = await open({ channelId: '' });
    const note = d.q('channel-secret-invalid')!;
    expect(note.getAttribute('role')).toBe('alert');
    expect(text(note)).toContain('without a channel');
    expect(d.el.querySelector('input')).toBeNull();
    expect(d.actions()).toEqual(['close']);
    await d.press('close');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
    expect(d.memory.secretOf('')).toBeUndefined();
  });

  it('does nothing on Enter without a secret, and Cancel dismisses it', async () => {
    const d = await open();
    await d.key('Enter');
    expect(d.outcome()).toBeNull();
    expect(d.memory.secretOf('slack')).toBeUndefined();
    await d.press('dismiss');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
  });
});
